//! Reading a body message by message, reserving before each is released.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use http::HeaderMap;
use http_body::{Body as HttpBody, Frame};
use norn_protocol::common::ErrorReason;
use norn_runtime::{Budget, BudgetError, Reservation};
use tonic::body::Body;
use tonic::{Code, Status};

use super::cost::{Settled, settle_batch, worst_batch_cost};
use super::wire::strip_samples;
use super::{Held, MeterLimits, StepResult};
use crate::status::error_info;

/// gRPC's message prefix: a compressed flag and a big-endian length.
pub(crate) const PREFIX: usize = 5;
/// The largest message of any method but `AppendSamples`. The largest legal one
/// is an annotated create: 64 entries of about 1.2 KiB.
const MAX_OTHER_BYTES: usize = 256 * 1024;
/// What prost can make of one encoded byte, at worst: a map entry of two empty
/// strings costs a few bytes on the wire and tens of bytes of `String`s and
/// table slots.
const EXPANSION: u64 = 32;
/// How long a message waits for decode budget.
const RESERVE_BOUND: Duration = Duration::from_secs(30);

/// One call's body. Its buffer holds at most what was already flow-controlled
/// in, so it needs no reservation of its own beyond the window.
pub(super) struct Reader {
    body: Body,
    buf: BytesMut,
    trailers: Option<HeaderMap>,
    ended: bool,
    budget: Budget,
    limits: MeterLimits,
    append: bool,
    held: Arc<Held>,
}

fn too_large(limit: usize, length: usize) -> Status {
    error_info(
        Code::ResourceExhausted,
        ErrorReason::LimitExceeded,
        format!("a {length}-byte message exceeds the limit of {limit}"),
        [("limit", limit.to_string())],
    )
}

fn no_room(e: &BudgetError) -> Status {
    let (code, reason) = match e {
        BudgetError::Closed { .. } => (Code::Unavailable, ErrorReason::ShuttingDown),
        _ => (Code::ResourceExhausted, ErrorReason::CapacityExhausted),
    };
    error_info(code, reason, e.to_string(), [("limit", "ipc_bytes".into())])
}

impl Reader {
    pub(super) fn new(
        body: Body,
        budget: Budget,
        limits: MeterLimits,
        append: bool,
        held: &Arc<Held>,
    ) -> Self {
        Self {
            body,
            buf: BytesMut::new(),
            trailers: None,
            ended: false,
            budget,
            limits,
            append,
            held: Arc::clone(held),
        }
    }

    pub(super) async fn next_frame(mut self) -> (Self, StepResult) {
        let frame = self.step().await;
        (self, frame)
    }

    async fn pull(&mut self) -> Result<(), Status> {
        let body = &mut self.body;
        let frame = poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)).await;
        match frame {
            None => self.ended = true,
            Some(Err(status)) => return Err(status),
            Some(Ok(frame)) => match frame.into_data() {
                Ok(data) => self.buf.extend_from_slice(&data),
                Err(frame) => self.trailers = frame.into_trailers().ok(),
            },
        }
        Ok(())
    }

    async fn fill(&mut self, wanted: usize) -> Result<bool, Status> {
        while self.buf.len() < wanted && !self.ended {
            self.pull().await?;
        }
        Ok(self.buf.len() >= wanted)
    }

    /// Owns its budget handle, so this future does not borrow the `!Sync` body.
    fn reserve(&self, bytes: u64) -> impl Future<Output = Result<Reservation, Status>> + use<> {
        let budget = self.budget.clone();
        async move {
            let reserved = budget.reserve(bytes, RESERVE_BOUND).await;
            reserved.map_err(|e| no_room(&e))
        }
    }

    fn remainder(&mut self) -> StepResult {
        if !self.buf.is_empty() {
            return Some(Ok(Frame::data(self.buf.split().freeze())));
        }
        self.trailers.take().map(|t| Ok(Frame::trailers(t)))
    }

    async fn step(&mut self) -> StepResult {
        match self.message().await {
            Ok(Some(data)) => Some(Ok(Frame::data(data))),
            Ok(None) => self.remainder(),
            Err(status) => {
                self.ended = true;
                self.buf.clear();
                Some(Err(status))
            }
        }
    }

    /// The next whole message with its prefix, paid for; `None` at the end.
    async fn message(&mut self) -> Result<Option<Bytes>, Status> {
        // A streaming handler asks for the next message only when done with the
        // last, so an idle call pins nothing. tonic drains a unary body before
        // its handler runs, so a unary reservation stays until the call ends.
        if self.append {
            self.held.replace(None);
        }
        if !self.fill(PREFIX).await? {
            self.held.replace(None);
            return Ok(None);
        }
        let mut word = [0u8; 4];
        if let Some(bytes) = self.buf.get(1..PREFIX) {
            word.copy_from_slice(bytes);
        }
        let length = u32::from_be_bytes(word) as usize;
        let limit = if self.append {
            self.limits.max_batch_bytes
        } else {
            MAX_OTHER_BYTES
        };
        if length > limit {
            return Err(too_large(limit, length));
        }
        // One reservation, for the worst case, taken whole: two would hold one
        // while waiting for the other, and calls doing that deadlock each other
        // until the bound. It can never exceed what the budget leaves decoding.
        let worst = if self.append {
            worst_batch_cost(length, self.limits)
        } else {
            EXPANSION * length as u64
        };
        let pool = self.budget.capacity() / 2;
        self.held.replace(None);
        let mut reservation = self.reserve(worst.min(pool).max(1)).await?;
        if !self.fill(PREFIX + length).await? {
            self.held.replace(Some(reservation));
            return Ok(None);
        }
        let whole = self.buf.split_to(PREFIX + length).freeze();
        if !self.append {
            self.held.replace(Some(reservation));
            return Ok(Some(whole));
        }
        let payload = whole.slice(PREFIX..);
        let settled = settle_batch(&payload, &mut reservation, self.limits);
        self.held.replace(Some(reservation));
        if let Settled::Oversized(samples) = settled {
            self.held.set_oversized(samples);
            return Ok(Some(strip_samples(&payload)));
        }
        Ok(Some(whole))
    }
}
