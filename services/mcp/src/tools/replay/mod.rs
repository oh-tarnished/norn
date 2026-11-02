//! replay: a session driven one bounded call at a time.
//!
//! Every session is created frozen, so nothing is emitted but by a step. Sessions are
//! recorded in [`Sessions`], which is why an agent can reach only its own: this
//! surface reads, so it must not stop a replay someone else is watching.

mod drive;
mod step;
mod table;

use norn_protocol::google::r#type::Interval;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{
    CreateReplaySessionRequest, DeleteReplaySessionRequest, GetReplaySessionRequest, ReplaySession,
    ReplayState,
};
use serde_json::{Value, json};

pub(crate) use table::Sessions;

use super::args_more::{Replay, ReplayAction as Action};
use super::parse;
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::{DEFAULT_LIMIT, MAX_LIMIT};
use crate::render::when;
use crate::select::{instant, selection};

const METHOD: &str = "ReplayService";

/// A session as a result shows it.
pub(crate) fn show(s: &ReplaySession) -> Value {
    let state = ReplayState::try_from(s.state).unwrap_or(ReplayState::Unspecified);
    let interval = s.interval.unwrap_or_default();
    json!({
        "session": s.name,
        "state": state.as_str_name().trim_start_matches("REPLAY_STATE_"),
        "clock": when(s.current_time),
        "samples_emitted": s.emitted_count,
        "from": when(interval.start_time),
        "to": when(interval.end_time),
        "expires": when(s.expire_time),
    })
}

fn needed<'a>(value: Option<&'a str>, action: &str, argument: &str) -> Result<&'a str, ToolError> {
    value.ok_or_else(|| ToolError::invalid(argument, format!("`{action}` needs it")))
}

async fn create(call: &Call, sessions: &Sessions, args: &Replay) -> Result<Value, ToolError> {
    let target = needed(args.target.as_deref(), "start", "target")?;
    let chosen = selection(target, args.select.as_deref())?;
    let from = instant(
        "from",
        needed(args.from.as_deref(), "start", "from")?,
        call.now,
    )?;
    let to = instant("to", needed(args.to.as_deref(), "start", "to")?, call.now)?;
    sessions.room(call).await?;
    let session = ReplaySession {
        interval: Some(Interval {
            start_time: Some(from),
            end_time: Some(to),
        }),
        scope: chosen.parent,
        filter: chosen.filter,
        pace: Some(Pace::Speed(0.0)),
        ..ReplaySession::default()
    };
    let request = CreateReplaySessionRequest {
        replay_session_id: String::new(),
        replay_session: Some(session),
    };
    let mut client = call.replays();
    let created = client.create_replay_session(request);
    let created = call.rpc(METHOD, created).await?;
    sessions.add(&created.name);
    Ok(show(&created))
}

pub(crate) async fn get(call: &Call, name: &str) -> Result<ReplaySession, ToolError> {
    let mut client = call.replays();
    let request = GetReplaySessionRequest {
        name: name.to_owned(),
    };
    call.rpc(METHOD, client.get_replay_session(request)).await
}

pub(crate) async fn delete(call: &Call, name: &str) -> Result<(), ToolError> {
    let mut client = call.replays();
    let request = DeleteReplaySessionRequest {
        name: name.to_owned(),
        etag: String::new(),
    };
    call.rpc(METHOD, client.delete_replay_session(request))
        .await
        .map(drop)
}

/// The tool.
pub(super) async fn replay(
    call: &Call,
    sessions: &Sessions,
    raw: &Value,
) -> Result<Value, ToolError> {
    let args: Replay = parse(raw)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ToolError::invalid("limit", format!("is 1 to {MAX_LIMIT}")));
    }
    let limit = limit as usize;
    if args.action == Action::List {
        return Ok(json!({"sessions": sessions.names()}));
    }
    if args.action == Action::Start {
        return create(call, sessions, &args).await;
    }
    let name = needed(args.session.as_deref(), "this action", "session")?;
    sessions.owned(name)?;
    match args.action {
        Action::Status => Ok(show(&get(call, name).await?)),
        Action::Stop => {
            let done = delete(call, name).await;
            sessions.remove(name);
            done.map(|()| json!({"session": name, "state": "STOPPED"}))
        }
        Action::Seek => {
            let time = needed(args.time.as_deref(), "seek", "time")?;
            let time = instant("time", time, call.now)?;
            drive::seek(call, name, time, limit).await
        }
        _ => step::step(call, name, args.events, args.duration.as_deref(), limit).await,
    }
}
