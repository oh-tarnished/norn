//! `DriveReplaySession`: move a frozen session's clock and return what it emitted.
//!
//! Events reach only a subscription attached when they are emitted, so a drive
//! attaches, moves the clock, and reads until a barrier: setting the pace to 0
//! again, which keeps the session frozen and makes it send `SPEED_CHANGED` after
//! everything the movement emitted. Reading to that event is reading all of it,
//! however many state events a seek brings, with no timing involved.

use std::sync::Arc;
use std::time::Duration;

use norn_core::Timestamp;
use norn_metadata::Registry;
use norn_protocol::common::ErrorReason;
use norn_protocol::replay::drive_replay_session_request::Movement;
use norn_protocol::replay::replay_event::Payload as WirePayload;
use norn_protocol::replay::{DriveReplaySessionRequest, DriveReplaySessionResponse, ReplayEvent};
use norn_runtime::query::{Engine, Function, Output, QuerySpec, Reduction, TimeRange};
use norn_runtime::replay::{ClockKind, Control, Pace, Payload};
use norn_runtime::sessions::{Replays, SessionInfo};
use tonic::{Code, Status};

use super::request::time;
use super::wire::{generation, id_of, session, status};
use crate::convert::{sample_to_wire, time_to_wire};
use crate::query::pull::{Item, cannot, cut, each, open, page_size, value_bytes};
use crate::query::request::{sample_filter, targets};
use crate::status::{bad_field, error_info};

/// The default and largest number of events one drive returns.
const DEFAULT_EVENTS: usize = 100;
const MAX_EVENTS: usize = 1000;

fn too_many(limit: usize) -> Status {
    let message = format!(
        "the movement would emit more than max_events ({limit}): step fewer events or a \
         shorter duration, or read the stretch with BrowseSamples"
    );
    let meta = [("limit", "max_events".to_owned())];
    error_info(
        Code::InvalidArgument,
        ErrorReason::LimitExceeded,
        message,
        meta,
    )
}

/// How many samples stepping `info`'s clock by `by` would emit, counted with the
/// session's own selection, so a step too big to return is refused before it moves.
fn pending(
    registry: &Registry,
    engine: &Arc<Engine>,
    info: &SessionInfo,
    by: Duration,
) -> Result<u64, Status> {
    let from = info.current_time;
    let nanos = i64::try_from(by.as_nanos()).unwrap_or(i64::MAX);
    let to = Timestamp::from_nanos(from.as_nanos().saturating_add(nanos)).min(info.end);
    let spec = QuerySpec {
        targets: targets(registry, &info.scope, &info.filter)?,
        range: TimeRange {
            start: Some(from),
            end: Some(to),
        },
        filter: sample_filter("")?,
        output: Output::Aggregates(Reduction {
            function: Function::Count,
            window: None,
        }),
        resume: None,
    };
    let mut total = 0;
    each(&mut open(engine, spec)?, |item| {
        if let Item::Aggregate(a) = item {
            total += a.samples;
        }
        Ok(true)
    })?;
    Ok(total)
}

/// The control `movement` asks for, refused before anything moves if it is too big.
async fn control(
    (registry, engine): (&Arc<Registry>, &Arc<Engine>),
    info: &SessionInfo,
    movement: Option<Movement>,
    limit: usize,
) -> Result<Control, Status> {
    Ok(match movement {
        Some(Movement::EventCount(n)) if n <= 0 => {
            return Err(bad_field("event_count", "must be positive"));
        }
        Some(Movement::EventCount(n)) if n as usize > limit => return Err(too_many(limit)),
        Some(Movement::EventCount(n)) => Control::StepEvents(n as u64),
        Some(Movement::Duration(d)) => {
            let nanos = i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos);
            let nanos = u64::try_from(nanos).ok().filter(|n| *n > 0);
            let by = Duration::from_nanos(
                nanos.ok_or_else(|| bad_field("duration", "must be positive"))?,
            );
            let (r, e, i) = (Arc::clone(registry), Arc::clone(engine), info.clone());
            let count = tokio::task::spawn_blocking(move || pending(&r, &e, &i, by));
            let count = count
                .await
                .map_err(|_| Status::internal("counting a step panicked"))??;
            if count > limit as u64 {
                return Err(too_many(limit));
            }
            Control::StepTime(by)
        }
        Some(Movement::SeekTime(t)) => Control::Seek {
            to: time("seek_time", Some(t))?,
            report_state: true,
        },
        None => {
            let why = "event_count, duration or seek_time is required";
            return Err(bad_field("movement", why));
        }
    })
}

/// Drives `request`'s session through `replays`, counting a duration step with
/// `engine` over `registry`'s signals first.
///
/// # Errors
/// `FAILED_PRECONDITION` with `ERROR_REASON_REPLAY_STATE_CONFLICT` for a session not
/// frozen, or one whose state forbids the movement; `INVALID_ARGUMENT` for a bad
/// field or a movement past `max_events`; `NOT_FOUND` for no such session.
pub(super) async fn drive(
    replays: &Replays,
    shared: (&Arc<Registry>, &Arc<Engine>),
    r: DriveReplaySessionRequest,
) -> Result<DriveReplaySessionResponse, Status> {
    let id = id_of(&r.name, "name")?;
    let limit = page_size("max_events", r.max_events, DEFAULT_EVENTS, MAX_EVENTS)?;
    let longest = value_bytes(r.max_value_bytes)?;
    let etag = generation(&r.etag)?;
    let info = replays.get(id).map_err(|e| status(&e, id))?;
    if !matches!(info.view.clock.pace(), Pace::Speed(s) if s == 0.0) {
        let message = "only a frozen session (speed 0) is driven; pause it and set speed 0";
        let meta = [("replay_session", r.name.clone())];
        let code = Code::FailedPrecondition;
        return Err(error_info(
            code,
            ErrorReason::ReplayStateConflict,
            message,
            meta,
        ));
    }
    let movement = control(shared, &info, r.movement, limit).await?;
    let mut subscription = replays.subscribe(id).await.map_err(|e| status(&e, id))?;
    let mut out = DriveReplaySessionResponse::default();
    let moved = async {
        replays.control(id, etag, movement).await?;
        replays
            .control(id, None, Control::SetPace(Pace::Speed(0.0)))
            .await
    };
    let moved = async { moved.await.map_err(|e| status(&e, id)) };
    let reading = async {
        while let Ok(batch) = subscription.next().await {
            for e in &batch.events {
                let (state, signal, sample) = match &e.payload {
                    Payload::Clock(ClockKind::SpeedChanged) => return Ok(()),
                    Payload::Sample { signal, sample } => (false, signal, sample),
                    Payload::State { signal, sample } => (true, signal, sample),
                    _ => continue,
                };
                if out.replay_events.len() >= limit {
                    out.omitted_event_count += 1;
                    continue;
                }
                let mut wire = sample_to_wire(sample, signal.to_string()).map_err(cannot)?;
                let value_truncated = cut(wire.value.as_mut(), longest);
                let payload = if state {
                    WirePayload::StateSample(wire)
                } else {
                    WirePayload::Sample(wire)
                };
                out.replay_events.push(ReplayEvent {
                    clock_time: Some(time_to_wire(e.clock_time)),
                    payload: Some(payload),
                    value_truncated,
                });
            }
        }
        Ok::<(), Status>(())
    };
    let (after, ()) = tokio::try_join!(moved, reading)?;
    out.replay_session = Some(session(&after));
    Ok(out)
}
