//! Moving a session's clock and reading what it emits.
//!
//! Events arrive only on a subscription, and only to one attached when they are
//! emitted, so each call subscribes, drives, and reads until a barrier: a pace change
//! that keeps the session frozen but sends `SPEED_CHANGED` after everything the
//! control emitted. So a call ends knowing it has seen all of it, whether a step's
//! samples or the state samples of a seek, however many those are.

use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{ReplaySession, SeekReplaySessionRequest, UpdateReplaySessionRequest};
use norn_protocol::telemetry::{ClockEventKind, SubscribeEventsRequest, SubscribeEventsResponse};
use norn_protocol::telemetry::{SubscriptionMode, event};
use prost_types::{FieldMask, Timestamp};
use serde_json::{Value, json};
use std::future::Future;
use tokio::time::timeout_at;
use tonic::Streaming;

use super::{METHOD, show};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::Rows;
use crate::render;

const SUBSCRIBE: &str = "TelemetryService.SubscribeEvents";

/// What a call saw. `omitted` is what fell past `limit` or the byte bound: only a
/// late sample or a seek across many signals, and it is reported, never hidden.
struct Seen {
    rows: Rows,
    omitted: u64,
    ended: bool,
}

fn row(payload: event::Payload) -> Option<Value> {
    let tagged = |s, kind: &str| {
        let mut row = render::sample(s);
        render::set(&mut row, "event", json!(kind));
        row
    };
    Some(match payload {
        event::Payload::Sample(s) => tagged(&s, "sample"),
        event::Payload::StateSample(s) => tagged(&s, "state"),
        event::Payload::ClockEvent(_) => return None,
    })
}

/// Reads to the barrier. Keeps reading past the bound, since the clock waits for its
/// slowest subscriber; only what it keeps is bounded.
async fn read(
    mut stream: Streaming<SubscribeEventsResponse>,
    seen: &mut Seen,
) -> Result<(), ToolError> {
    loop {
        let page = stream.message().await;
        let Some(page) = page.map_err(|s| ToolError::daemon(SUBSCRIBE, &s))? else {
            return Ok(());
        };
        for event in page.events {
            let Some(payload) = event.payload else {
                continue;
            };
            if let event::Payload::ClockEvent(clock) = &payload {
                match ClockEventKind::try_from(clock.kind) {
                    Ok(ClockEventKind::SpeedChanged) => return Ok(()),
                    Ok(ClockEventKind::Ended) => seen.ended = true,
                    _ => {}
                }
            }
            if let Some(row) = row(payload)
                && !seen.rows.push(row)
            {
                seen.omitted += 1;
            }
        }
    }
}

async fn barrier(call: &Call, name: &str) -> Result<ReplaySession, ToolError> {
    let session = ReplaySession {
        name: name.to_owned(),
        pace: Some(Pace::Speed(0.0)),
        ..ReplaySession::default()
    };
    let request = UpdateReplaySessionRequest {
        replay_session: Some(session),
        update_mask: Some(FieldMask {
            paths: vec!["speed".to_owned()],
        }),
    };
    let mut client = call.replays();
    call.rpc(METHOD, client.update_replay_session(request))
        .await
}

/// Subscribes, runs `control`, and reads what it emits.
pub(super) async fn drive(
    call: &Call,
    name: &str,
    limit: usize,
    control: impl Future<Output = Result<ReplaySession, ToolError>>,
) -> Result<Value, ToolError> {
    let request = SubscribeEventsRequest {
        mode: SubscriptionMode::Replay as i32,
        replay_session: name.to_owned(),
        ..SubscribeEventsRequest::default()
    };
    let mut client = call.telemetry();
    let stream = call
        .rpc(SUBSCRIBE, client.subscribe_events(request))
        .await?;
    let mut seen = Seen {
        rows: Rows::new(limit),
        omitted: 0,
        ended: false,
    };
    let moved = async {
        control.await?;
        barrier(call, name).await
    };
    let reading = read(stream, &mut seen);
    let both = async { tokio::try_join!(moved, reading) };
    let (session, ()) = timeout_at(call.deadline, both)
        .await
        .map_err(|_| ToolError::deadline())??;
    let mut result = show(&session);
    render::set(&mut result, "ended", json!(seen.ended));
    render::set(&mut result, "omitted", json!(seen.omitted));
    let rows = seen.rows.into_rows();
    render::set(&mut result, "count", json!(rows.len()));
    render::set(&mut result, "events", Value::Array(rows));
    Ok(result)
}

/// Moves the clock to `time`; the events are each signal's state just before it.
pub(super) async fn seek(
    call: &Call,
    name: &str,
    time: Timestamp,
    limit: usize,
) -> Result<Value, ToolError> {
    let request = SeekReplaySessionRequest {
        name: name.to_owned(),
        seek_time: Some(time),
        etag: String::new(),
    };
    let mut client = call.replays();
    let control = call.rpc(METHOD, client.seek_replay_session(request));
    drive(call, name, limit, control).await
}
