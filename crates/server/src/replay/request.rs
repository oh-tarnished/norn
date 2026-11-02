//! Reading replay requests into what the runtime takes.

use std::time::Duration;

use norn_core::Timestamp;
use norn_metadata::Registry;
use norn_protocol::replay::replay_session::Pace as WirePace;
use norn_protocol::replay::step_replay_session_request::Step;
use norn_protocol::replay::{ReplaySession, StepReplaySessionRequest};
use norn_runtime::replay::{Control, Pace, SessionSpec};
use tonic::Status;

use crate::convert::time_from_wire;
use crate::query::request::targets;
use crate::status::bad_field;

/// A pace from the wire; unset is real time.
///
/// # Errors
/// `INVALID_ARGUMENT` for a negative or non-finite speed, or `max_speed = false`.
pub(crate) fn pace(wire: Option<WirePace>, field: &str) -> Result<Pace, Status> {
    match wire {
        None => Ok(Pace::default()),
        Some(WirePace::MaxSpeed(true)) => Ok(Pace::Maximum),
        Some(WirePace::MaxSpeed(false)) => Err(bad_field(field, "max_speed may only be true")),
        Some(WirePace::Speed(s)) => {
            Pace::speed(s).ok_or_else(|| bad_field(field, "speed must be finite and not negative"))
        }
    }
}

/// A timestamp field that must be set.
pub(crate) fn time(field: &str, t: Option<prost_types::Timestamp>) -> Result<Timestamp, Status> {
    let t = t.ok_or_else(|| bad_field(field, "is required"))?;
    time_from_wire(t).map_err(|e| bad_field(field, &e.to_string()))
}

/// What a new session replays. Resolves its signals now: one created later does
/// not join. Blocks on nothing but the registry's locks.
///
/// # Errors
/// `INVALID_ARGUMENT` for a missing or reversed interval, a bad pace or filter;
/// `NOT_FOUND` for an unknown entity.
pub(crate) fn spec(registry: &Registry, s: &ReplaySession) -> Result<SessionSpec, Status> {
    let interval = s
        .interval
        .ok_or_else(|| bad_field("replay_session.interval", "is required"))?;
    let start = time("replay_session.interval.start_time", interval.start_time)?;
    let end = time("replay_session.interval.end_time", interval.end_time)?;
    if start > end {
        return Err(bad_field(
            "replay_session.interval",
            "must not end before it starts",
        ));
    }
    if s.scope.is_empty() {
        return Err(bad_field("replay_session.scope", "is required"));
    }
    Ok(SessionSpec {
        start,
        end,
        targets: targets(registry, &s.scope, &s.filter)?,
        pace: pace(s.pace, "replay_session.pace")?,
    })
}

/// A step from the wire.
///
/// # Errors
/// `INVALID_ARGUMENT` when neither or a non-positive amount is given.
pub(crate) fn step(r: &StepReplaySessionRequest) -> Result<Control, Status> {
    match r.step {
        Some(Step::EventCount(n)) if n > 0 => Ok(Control::StepEvents(n as u64)),
        Some(Step::EventCount(_)) => Err(bad_field("event_count", "must be positive")),
        Some(Step::Duration(d)) => {
            let nanos = i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos);
            let nanos = u64::try_from(nanos).ok().filter(|n| *n > 0);
            let nanos = nanos.ok_or_else(|| bad_field("duration", "must be positive"))?;
            Ok(Control::StepTime(Duration::from_nanos(nanos)))
        }
        None => Err(bad_field("step", "event_count or duration is required")),
    }
}

/// The pace an update asks for. AIP-134: with no mask, whichever pace is set;
/// a mask names `speed` or `max_speed`, and the named one must be set.
///
/// # Errors
/// `INVALID_ARGUMENT` for any other path, or no pace at all.
pub(crate) fn new_pace(
    s: &ReplaySession,
    mask: Option<prost_types::FieldMask>,
) -> Result<Pace, Status> {
    let paths = mask.map(|m| m.paths).unwrap_or_default();
    let field = "update_mask";
    for path in &paths {
        let named = match (path.as_str(), s.pace) {
            ("speed", Some(WirePace::Speed(_))) | ("max_speed", Some(WirePace::MaxSpeed(_))) => {
                true
            }
            ("speed" | "max_speed", _) => return Err(bad_field(field, "names a pace not set")),
            _ => false,
        };
        if !named {
            return Err(bad_field(field, "only speed or max_speed can change"));
        }
    }
    if s.pace.is_none() {
        return Err(bad_field("replay_session.pace", "is required"));
    }
    pace(s.pace, "replay_session.pace")
}
