//! Deciding a step: how far, and refusing one too big to return before anything moves.

use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest};
use norn_protocol::replay::step_replay_session_request::Step;
use norn_protocol::replay::{ReplaySession, StepReplaySessionRequest};
use serde_json::Value;

use super::drive::drive;
use super::{METHOD, get};
use crate::cursor::{Cursor, Order};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::select::duration as parse_duration;
use crate::time;
use crate::walk::{Item, walk};

/// How many samples moving the clock by `nanos` would emit, per the session's own
/// selection, so a step too big to return is refused before anything moves.
async fn pending(call: &Call, session: &ReplaySession, nanos: i128) -> Result<i64, ToolError> {
    let interval = session.interval.unwrap_or_default();
    let clock = session.current_time.unwrap_or_default();
    let end = time::timestamp(time::nanos(clock) + nanos);
    let end = end.ok_or_else(|| ToolError::invalid("duration", "this duration is too long"))?;
    let last = interval.end_time.unwrap_or_default();
    let end = if time::nanos(end) < time::nanos(last) {
        end
    } else {
        last
    };
    let request = QuerySamplesRequest {
        parent: session.scope.clone(),
        filter: session.filter.clone(),
        interval: Some(norn_protocol::google::r#type::Interval {
            start_time: Some(clock),
            end_time: Some(end),
        }),
        aggregation: Some(Aggregation {
            aggregate_function: AggregateFunction::Count as i32,
            window: None,
        }),
        ..QuerySamplesRequest::default()
    };
    let mut total = 0;
    let start = Cursor::default();
    walk(call, request, Order::Signal, &start, |item| {
        if let Item::Aggregate(a) = item {
            total += a.sample_count;
        }
        true
    })
    .await?;
    Ok(total)
}

/// Emits the next `events`, or `duration` of virtual time, returning what was emitted.
pub(super) async fn step(
    call: &Call,
    name: &str,
    events: Option<u32>,
    duration: Option<&str>,
    limit: usize,
) -> Result<Value, ToolError> {
    let hint = "Step fewer `events` or a shorter `duration`, or raise `limit`; or read the \
                interval with query_range.";
    let step = match (events, duration) {
        (Some(0), None) => {
            return Err(ToolError::invalid("events", "must be at least 1"));
        }
        (Some(n), None) if n as usize > limit => {
            return Err(ToolError::limit("the step's limit", limit, hint));
        }
        (Some(n), None) => Step::EventCount(n as i32),
        (None, Some(text)) => {
            let nanos = parse_duration("duration", text)?;
            let session = get(call, name).await?;
            if pending(call, &session, nanos).await? > limit as i64 {
                return Err(ToolError::limit("the step's limit", limit, hint));
            }
            let wire = time::wire_duration(nanos);
            Step::Duration(
                wire.ok_or_else(|| ToolError::invalid("duration", "this duration is too long"))?,
            )
        }
        _ => {
            return Err(ToolError::invalid(
                "events",
                "give `events` or `duration`, one of them",
            ));
        }
    };
    let request = StepReplaySessionRequest {
        name: name.to_owned(),
        step: Some(step),
        etag: String::new(),
    };
    let mut client = call.replays();
    let control = call.rpc(METHOD, client.step_replay_session(request));
    drive(call, name, limit, control).await
}
