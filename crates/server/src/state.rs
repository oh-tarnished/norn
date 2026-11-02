//! `norn.state.v1.StateService`: `ComputeState`, SPEC's `StateAt(T)`.

use std::sync::Arc;

use norn_core::Timestamp;
use norn_metadata::Registry;
use norn_protocol::common::ErrorReason;
use norn_protocol::state::state_service_server::StateService;
use norn_protocol::state::{ComputeStateRequest, ComputeStateResponse, SignalState as WireState};
use norn_runtime::Clock;
use norn_runtime::query::Engine;
use norn_runtime::state::{PinId, Pins, SignalState, State, StateReader, page_length};
use prost::Message;
use tonic::{Code, Request, Response, Status};

use crate::convert::{sample_to_wire, time_from_wire, time_to_wire};
use crate::query::reply::status;
use crate::query::request::targets;
use crate::status::{bad_field, error_info};

/// The token format's name, changed if it ever changes meaning.
const TOKEN: &str = "s1";
const DEFAULT_PAGE: usize = 1_000;
const MAX_PAGE: usize = 10_000;
/// Roughly the most encoded bytes one response aims at; see `norn_query`'s pages.
const PAGE_BYTES: u64 = 2 << 20;

/// The StateService implementation.
///
/// Concurrency: `Clone`, shared by tonic. Every read runs on the blocking pool, so a
/// slow disk holds no async thread. Memory: a state is charged to the query budget
/// while it is built, and for as long as [`Pins`] holds it. Time comes from the
/// injected clock, and only ages pins.
#[derive(Debug, Clone)]
pub struct States {
    registry: Arc<Registry>,
    reader: StateReader,
    pins: Arc<Pins>,
    clock: Arc<dyn Clock>,
}

impl States {
    /// A service selecting signals from `registry` and reading through `engine`.
    #[must_use]
    pub fn new(registry: Arc<Registry>, engine: Arc<Engine>, clock: Arc<dyn Clock>) -> Self {
        Self {
            registry,
            reader: StateReader::new(engine),
            pins: Arc::new(Pins::new()),
            clock,
        }
    }

    async fn compute(&self, r: &ComputeStateRequest) -> Result<Arc<State>, Status> {
        let at = r
            .state_time
            .ok_or_else(|| bad_field("state_time", "is required"))?;
        let at = time_from_wire(at).map_err(|e| bad_field("state_time", &e.to_string()))?;
        let max_age = r.max_age.map(age).transpose()?;
        let (registry, reader) = (Arc::clone(&self.registry), self.reader.clone());
        let (parent, filter) = (r.parent.clone(), r.filter.clone());
        tokio::task::spawn_blocking(move || {
            let selected = targets(&registry, &parent, &filter)?;
            let state = reader.state_at(selected, at, max_age);
            state.map(Arc::new).map_err(|e| status(&e))
        })
        .await
        .map_err(|_| Status::internal("a state computation panicked"))?
    }
}

fn age(d: prost_types::Duration) -> Result<i64, Status> {
    let nanos = i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos);
    let bad = || bad_field("max_age", "must not be negative, and must fit 292 years");
    i64::try_from(nanos)
        .ok()
        .filter(|n| *n >= 0)
        .ok_or_else(bad)
}

/// Identifies a request, its token and page size aside (AIP-158 lets the page size
/// change between pages). FNV-1a; a fingerprint and not a secret.
fn fingerprint(r: &ComputeStateRequest) -> u64 {
    let bare = ComputeStateRequest {
        page_size: 0,
        page_token: String::new(),
        ..r.clone()
    };
    bare.encode_to_vec()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

fn page_size(requested: i32) -> Result<usize, Status> {
    match usize::try_from(requested) {
        Ok(0) => Ok(DEFAULT_PAGE),
        Ok(n) => Ok(n.min(MAX_PAGE)),
        Err(_) => Err(bad_field("page_size", "must not be negative")),
    }
}

fn bad_token() -> Status {
    let text = "the page token is malformed, has expired, or belongs to another request";
    error_info(Code::InvalidArgument, ErrorReason::InvalidToken, text, [])
}

fn parse_token(text: &str, print: u64) -> Option<(PinId, usize)> {
    let rest = text.strip_prefix(&format!("{TOKEN}.{print:016x}."))?;
    let (pin, offset) = rest.split_once('.')?;
    Some((PinId(pin.parse().ok()?), offset.parse().ok()?))
}

fn wire(entry: &SignalState) -> Result<WireState, Status> {
    let cannot = |e| Status::internal(format!("cannot send a stored value: {e}"));
    let name = entry.signal.to_string();
    let sample = entry.sample.as_ref();
    let sample = sample.map(|s| sample_to_wire(s, name.clone()).map_err(cannot));
    Ok(WireState {
        signal: name,
        sample: sample.transpose()?,
    })
}

fn response(
    entries: &[SignalState],
    at: Timestamp,
    next_page_token: String,
) -> Result<ComputeStateResponse, Status> {
    Ok(ComputeStateResponse {
        signal_states: entries.iter().map(wire).collect::<Result<_, _>>()?,
        state_time: Some(time_to_wire(at)),
        next_page_token,
    })
}

#[tonic::async_trait]
impl StateService for States {
    async fn compute_state(
        &self,
        request: Request<ComputeStateRequest>,
    ) -> Result<Response<ComputeStateResponse>, Status> {
        let r = request.into_inner();
        let size = page_size(r.page_size)?;
        let print = fingerprint(&r);
        let now = self.clock.now();
        let (state, pin, offset) = if r.page_token.is_empty() {
            (self.compute(&r).await?, None, 0)
        } else {
            let (pin, offset) = parse_token(&r.page_token, print).ok_or_else(bad_token)?;
            let held = self.pins.get(pin, print, now).ok_or_else(bad_token)?;
            (held, Some(pin), offset)
        };
        let rest = state.entries().get(offset..).ok_or_else(bad_token)?;
        let length = page_length(rest, size, PAGE_BYTES);
        let page = rest.get(..length).unwrap_or(rest);
        let next = offset + length;
        let token = if next < state.entries().len() {
            let pin = pin.unwrap_or_else(|| self.pins.pin(Arc::clone(&state), print, now));
            format!("{TOKEN}.{print:016x}.{pin}.{next}")
        } else {
            if let Some(pin) = pin {
                self.pins.release(pin);
            }
            String::new()
        };
        Ok(Response::new(response(page, state.at, token)?))
    }
}
