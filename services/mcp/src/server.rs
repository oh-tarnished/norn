//! The MCP server: the tool list and calls, over any transport.

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::time::timeout_at;

use crate::daemon::Daemon;
use crate::error::ToolError;
use crate::tools::{self, Sessions};

/// The most tool calls answered at once, across all clients; others wait, within
/// their deadlines. Each holds a daemon page and a result in memory, so this is what
/// bounds this process's memory. Picked, not measured.
const MAX_CONCURRENT_CALLS: usize = 8;

const INSTRUCTIONS: &str = "Norn stores the history of systems as entities, their signals, and \
    timestamped samples of those signals. These tools read it: start with list_entities and \
    list_signals. Every tool is a deterministic temporal primitive; none interprets anything. \
    Times are RFC 3339 in UTC, Unix seconds, `now` or `now-1h`, and intervals are half-open. \
    Results are bounded: when `complete` is false, repeat the call with `continuation`. A failed \
    call returns a JSON `error` with a stable `reason`, and usually a `hint`.";

/// The protocol revisions offered: as bacnet-rs's server, up to the last whose results
/// clients have been seen to accept in full.
const PROTOCOLS: &[ProtocolVersion] = &[
    ProtocolVersion::V_2024_11_05,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
];

/// The server. `new` does no I/O; a transport runs it; [`Self::close`] releases what it
/// opened in the daemon.
///
/// Concurrency: `Clone`, and clones share everything, which is how each streamable
/// HTTP session gets its own handler over one table of replay sessions.
#[derive(Debug, Clone)]
pub struct NornMcp {
    daemon: Daemon,
    sessions: Sessions,
    gate: Arc<Semaphore>,
}

impl NornMcp {
    /// A server answering from `daemon`.
    #[must_use]
    pub fn new(daemon: Daemon) -> Self {
        Self {
            daemon,
            sessions: Sessions::default(),
            gate: Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS)),
        }
    }

    /// Stops every replay session still open. Idempotent; safe after a failure.
    pub async fn close(&self) {
        self.sessions.close(&self.daemon.call()).await;
    }

    async fn answer(&self, name: &str, raw: &Value) -> Option<Result<Value, ToolError>> {
        let call = self.daemon.call();
        let Ok(Ok(_permit)) = timeout_at(call.deadline, self.gate.acquire()).await else {
            return Some(Err(ToolError::deadline()));
        };
        tools::dispatch(&call, &self.sessions, name, raw).await
    }
}

fn text(value: &Value) -> Vec<ContentBlock> {
    vec![ContentBlock::text(value.to_string())]
}

impl ServerHandler for NornMcp {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(PROTOCOLS)
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("norn-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(tools::tools())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let raw = Value::Object(request.arguments.unwrap_or_default());
        let answered = tokio::select! {
            answered = self.answer(&request.name, &raw) => answered,
            () = context.ct.cancelled() => return Err(ErrorData::internal_error("cancelled", None)),
        };
        let Some(answered) = answered else {
            let message = format!("no tool {}", request.name);
            return Err(ErrorData::invalid_params(message, None));
        };
        Ok(match answered {
            Ok(value) => CallToolResult::success(text(&value)),
            Err(e) => CallToolResult::error(text(&e.to_json())),
        }
        .into())
    }
}
