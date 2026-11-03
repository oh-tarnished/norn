//! Every generated `<Service>McpServer` trait, implemented once.
//!
//! This is the whole hand-written tool surface. A read is its RPC by name: the
//! schema, the description and the hints are generated, and the answer is the RPC's.
//! A method without a tool has no path to the daemon at all: its arm here is a
//! refusal, so read-only holds by construction, not by a check that could be missed.

use std::sync::Arc;
use std::time::Duration;

use rmcp::ErrorData as McpError;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tonic::{Code, Status};

use super::dial::Dial;
use super::errors::{failure, no_tool, refused};
use super::generated::{admin, entity, query, replay, signal, state, stream};
use super::rpc;
use super::sessions::{MAX_SESSIONS, Sessions};

/// Tool calls at once, across every client; the rest wait within their deadlines.
const CALLS: usize = 8;
/// The deadline of one tool call.
const DEADLINE: Duration = Duration::from_secs(30);
const REPLAY: &str = "norn.replay.v1.ReplayService";

/// The services, as every generated trait wants them. Cheap to clone; correct from
/// any task.
#[derive(Clone)]
pub(crate) struct Bridge(Arc<Inner>);

struct Inner {
    dial: Dial,
    calls: Semaphore,
    sessions: Sessions,
}

/// `get_entity` as the schema spells it, `GetEntity`.
fn pascal(snake: &str) -> String {
    let word = |w: &str| {
        let mut c = w.chars();
        let first = c.next().map(|f| f.to_ascii_uppercase());
        first.into_iter().chain(c).collect::<String>()
    };
    snake.split('_').map(word).collect()
}

impl Bridge {
    pub(crate) fn new(dial: Dial) -> Self {
        Self(Arc::new(Inner {
            dial,
            calls: Semaphore::new(CALLS),
            sessions: Sessions::default(),
        }))
    }

    async fn rpc(&self, service: &str, method: &str, args: Value) -> Result<Value, Status> {
        let loopback = self.0.dial.get().await?;
        rpc::call(loopback, service, &pascal(method), args).await
    }

    /// Forgets sessions the daemon has expired.
    async fn sweep(&self) {
        for name in self.0.sessions.names() {
            let args = json!({"name": name});
            let found = self.rpc(REPLAY, "get_replay_session", args).await;
            if found.is_err_and(|s| s.code() == Code::NotFound) {
                self.0.sessions.forget(&name);
            }
        }
    }

    /// Refuses a replay call this surface may not make.
    async fn admit(&self, method: &str, args: &Value) -> Result<(), McpError> {
        if method == "create_replay_session" {
            if self.0.sessions.count() >= MAX_SESSIONS {
                self.sweep().await;
            }
            if self.0.sessions.count() >= MAX_SESSIONS {
                let meta = json!({"limit": "replay sessions"});
                let text = "too many open replay sessions: delete one first";
                return Err(failure("MCP_LIMIT_EXCEEDED", text, false, meta));
            }
        }
        let named = matches!(
            method,
            "get_replay_session" | "drive_replay_session" | "delete_replay_session"
        );
        let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
        if named && !self.0.sessions.owns(name) {
            let text = "not a replay session a tool created, or it has been deleted";
            return Err(failure("MCP_UNKNOWN_SESSION", text, false, json!({})));
        }
        Ok(())
    }

    async fn call(&self, service: &str, method: &str, args: Value) -> Result<Value, McpError> {
        let replay = service == REPLAY;
        let name = args.get("name").and_then(Value::as_str).map(str::to_owned);
        let work = async {
            let _slot = self.0.calls.acquire().await;
            if replay {
                self.admit(method, &args).await?;
            }
            self.rpc(service, method, args).await.map_err(|s| {
                if s.code() == Code::NotFound
                    && let Some(n) = name.as_deref().filter(|_| replay)
                {
                    self.0.sessions.forget(n);
                }
                refused(&s)
            })
        };
        let mut answer = tokio::time::timeout(DEADLINE, work).await.map_err(|_| {
            failure(
                "MCP_DEADLINE_EXCEEDED",
                "the call reached its deadline",
                true,
                json!({}),
            )
        })??;
        match method {
            "create_replay_session" => {
                let made = answer.get("name").and_then(Value::as_str);
                made.into_iter().for_each(|n| self.0.sessions.add(n));
            }
            "delete_replay_session" => name.iter().for_each(|n| self.0.sessions.forget(n)),
            "list_replay_sessions" => self.0.sessions.filter(&mut answer),
            _ => {}
        }
        Ok(answer)
    }
}

macro_rules! bridge {
    ($($module:ident :: $trait:ident = $service:literal
        { reads: $($read:ident),* ; none: $($none:ident),* })*) => {
        $(
            #[async_trait::async_trait]
            impl $module::$trait for Bridge {
                $(async fn $read(&self, args: Value) -> Result<Value, McpError> {
                    self.call($service, stringify!($read), args).await
                })*
                $(async fn $none(&self, _: Value) -> Result<Value, McpError> {
                    Err(no_tool())
                })*
            }
        )*
    };
}

bridge! {
    admin::AdminServiceMcpServer = "norn.admin.v1.AdminService" {
        reads: get_server; none: negotiate_protocol
    }
    entity::EntityServiceMcpServer = "norn.entity.v1.EntityService" {
        reads: get_entity, list_entities;
        none: create_entity, batch_create_entities, update_entity, delete_entity
    }
    signal::SignalServiceMcpServer = "norn.signal.v1.SignalService" {
        reads: get_signal, list_signals;
        none: create_signal, batch_create_signals, update_signal, delete_signal
    }
    stream::StreamServiceMcpServer = "norn.stream.v1.StreamService" {
        reads: get_stream, list_streams;
        none: create_stream, update_stream, delete_stream, compact_stream, verify_stream,
            apply_stream_retention, export_stream
    }
    state::StateServiceMcpServer = "norn.state.v1.StateService" { reads: compute_state; none: }
    query::QueryServiceMcpServer = "norn.query.v1.QueryService" {
        reads: browse_samples, find_events, compute_stats, compare_periods; none:
    }
    replay::ReplayServiceMcpServer = "norn.replay.v1.ReplayService" {
        reads: create_replay_session, get_replay_session, list_replay_sessions,
            drive_replay_session, delete_replay_session;
        none: update_replay_session, pause_replay_session, resume_replay_session,
            seek_replay_session, step_replay_session, stop_replay_session
    }
}
