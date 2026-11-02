//! The tools: what each is called, what a model is told, and which code answers it.

mod args;
mod args_more;
mod catalog;
mod events;
mod read;
mod replay;
mod resources;
mod state;
mod stats;
mod summary;

use serde::de::DeserializeOwned;
use serde_json::Value;

pub use catalog::tools;
pub(crate) use replay::Sessions;

use crate::daemon::Call;
use crate::error::ToolError;

fn parse<T: DeserializeOwned>(raw: &Value) -> Result<T, ToolError> {
    serde_json::from_value(raw.clone()).map_err(|e| ToolError::invalid("arguments", e))
}

/// Runs tool `name` on `raw` arguments; `None` when there is no such tool.
pub(crate) async fn dispatch(
    call: &Call,
    sessions: &Sessions,
    name: &str,
    raw: &Value,
) -> Option<Result<Value, ToolError>> {
    Some(match name {
        "list_entities" => resources::list_entities(call, raw).await,
        "list_signals" => resources::list_signals(call, raw).await,
        "get_signal_metadata" => resources::get_signal(call, raw).await,
        "query_range" => read::query_range(call, raw).await,
        "get_latest" => read::get_latest(call, raw).await,
        "state_at" => state::state_at(call, raw).await,
        "find_events" => events::find_events(call, raw).await,
        "get_statistics" => stats::get_statistics(call, raw).await,
        "compare_periods" => stats::compare_periods(call, raw).await,
        "replay" => replay::replay(call, sessions, raw).await,
        _ => return None,
    })
}
