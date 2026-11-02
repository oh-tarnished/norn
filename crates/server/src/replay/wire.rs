//! Sessions and replay failures as the wire's.

use norn_protocol::common::ErrorReason;
use norn_protocol::google::r#type::Interval;
use norn_protocol::replay::replay_session::Pace as WirePace;
use norn_protocol::replay::{ReplaySession, ReplayState as WireState};
use norn_runtime::replay::{Pace, ReplayError, ReplayState};
use norn_runtime::sessions::SessionInfo;
use tonic::{Code, Status};

use crate::convert::time_to_wire;
use crate::query::reply::status as query_status;
use crate::status::{bad_field, error_info};

/// The resource name of session `id`.
pub(crate) fn name(id: &str) -> String {
    format!("replaySessions/{id}")
}

/// The session id in `name`.
///
/// # Errors
/// `INVALID_ARGUMENT` naming `field` when it is not `replaySessions/{id}`.
pub(crate) fn id_of<'a>(name: &'a str, field: &str) -> Result<&'a str, Status> {
    name.strip_prefix("replaySessions/")
        .filter(|id| valid_id(id))
        .ok_or_else(|| bad_field(field, "must be replaySessions/{replay_session}"))
}

/// AIP-122: lower-case letters, digits and hyphens, a letter first, at most 63.
pub(crate) fn valid_id(id: &str) -> bool {
    let shaped = id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let first = id.bytes().next().is_some_and(|b| b.is_ascii_lowercase());
    shaped && first && id.len() <= 63 && !id.ends_with('-')
}

/// The etag of a session at `generation`. A number and not a secret: it says only
/// that nothing changed.
pub(crate) fn etag(generation: u64) -> String {
    format!("\"g{generation}\"")
}

/// The generation an etag names, if one was given.
///
/// # Errors
/// `ABORTED` with `ERROR_REASON_ETAG_MISMATCH` for an etag never issued.
pub(crate) fn generation(etag: &str) -> Result<Option<u64>, Status> {
    if etag.is_empty() {
        return Ok(None);
    }
    let parsed = etag.strip_prefix("\"g").and_then(|e| e.strip_suffix('"'));
    let parsed = parsed.and_then(|n| n.parse().ok());
    parsed
        .map(Some)
        .ok_or_else(|| status(&ReplayError::EtagMismatch, ""))
}

fn state(s: ReplayState) -> WireState {
    match s {
        ReplayState::Paused => WireState::Paused,
        ReplayState::Running => WireState::Running,
        ReplayState::Ended => WireState::Ended,
        ReplayState::Stopped => WireState::Stopped,
        _ => WireState::Unspecified,
    }
}

/// `info` as the resource.
pub(crate) fn session(info: &SessionInfo) -> ReplaySession {
    let pace = match info.view.clock.pace() {
        Pace::Speed(speed) => WirePace::Speed(speed),
        Pace::Maximum => WirePace::MaxSpeed(true),
    };
    ReplaySession {
        name: name(&info.id),
        interval: Some(Interval {
            start_time: Some(time_to_wire(info.start)),
            end_time: Some(time_to_wire(info.end)),
        }),
        scope: info.scope.clone(),
        filter: info.filter.clone(),
        pace: Some(pace),
        state: state(info.view.state) as i32,
        current_time: Some(time_to_wire(info.current_time)),
        emitted_count: i64::try_from(info.view.emitted).unwrap_or(i64::MAX),
        create_time: Some(time_to_wire(info.created)),
        update_time: Some(time_to_wire(info.updated)),
        expire_time: Some(time_to_wire(info.expires)),
        etag: etag(info.view.generation),
    }
}

/// A replay failure as the protocol's status; `id` names the session.
pub(crate) fn status(e: &ReplayError, id: &str) -> Status {
    let text = e.to_string();
    let with = |code, reason, meta: Vec<(&'static str, String)>| {
        error_info(code, reason, text.clone(), meta)
    };
    match e {
        ReplayError::Conflict { .. } => with(
            Code::FailedPrecondition,
            ErrorReason::ReplayStateConflict,
            vec![],
        ),
        ReplayError::NotFound => with(
            Code::NotFound,
            ErrorReason::ResourceNotFound,
            vec![("resource", name(id))],
        ),
        ReplayError::AlreadyExists => with(
            Code::AlreadyExists,
            ErrorReason::ResourceAlreadyExists,
            vec![("resource", name(id))],
        ),
        ReplayError::TooManySessions { .. } => with(
            Code::ResourceExhausted,
            ErrorReason::CapacityExhausted,
            vec![("limit", "max_replay_sessions".to_owned())],
        ),
        ReplayError::EtagMismatch => with(Code::Aborted, ErrorReason::EtagMismatch, vec![]),
        ReplayError::OutsideInterval => bad_field("seek_time", &text),
        ReplayError::Query(e) => query_status(e),
        _ => with(Code::Unavailable, ErrorReason::ShuttingDown, vec![]),
    }
}
