//! One MCP endpoint over the generated handlers: only the tools annotated read-only
//! are listed or dispatched, whatever else the generator emitted, since protoc-gen-mcp
//! makes a tool of every unary RPC.
//!
//! A failed call is an `isError` result, so the model reads it; only an unknown tool
//! is a protocol error. Output schemas are withheld: the generated ones name fields
//! as the `.proto` spells them, not as the canonical JSON every reply is written in,
//! and a client validating them would reject every result.

use std::borrow::Cow;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
    Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::json;

use super::bridge::Bridge;
use super::dial::Dial;
use super::generated::{admin, entity, query, replay, signal, state, stream};

/// The revisions whose results rmcp sends in full; it would agree to `2026-07-28`
/// but omits fields that revision requires, and strict clients reject them.
const PROTOCOLS: &[ProtocolVersion] = &[
    ProtocolVersion::V_2024_11_05,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
];

const INSTRUCTIONS: &str = "Norn's history of a system: what exists, what each signal \
measured, its state at any instant, and replay. Every tool is a deterministic read; none \
changes stored data. Create a replay session with speed 0, drive it, then delete it.";

fn read_only(tool: &Tool) -> bool {
    let hints = tool.annotations.as_ref();
    hints.is_some_and(|a| a.read_only_hint == Some(true))
}

macro_rules! endpoint {
    ($($field:ident: $module:ident :: $handler:ident = $prefix:literal,)*) => {
        /// The combined endpoint. Cheap to clone, as each session needs it; correct
        /// from any task.
        #[derive(Clone)]
        pub(crate) struct Endpoint {
            $($field: $module::$handler<Bridge>,)*
        }

        impl Endpoint {
            pub(crate) fn new(dial: Dial) -> Self {
                let bridge = Bridge::new(dial);
                Self { $($field: $module::$handler::new(bridge.clone()),)* }
            }

            /// Every tool served: the read-only ones, from every service.
            pub(crate) fn tools() -> Vec<Tool> {
                let all = [$($module::$handler::<Bridge>::tools(),)*].concat();
                all.into_iter().filter(read_only).collect()
            }

            async fn dispatch(
                &self,
                request: CallToolRequestParams,
                context: RequestContext<RoleServer>,
            ) -> Result<CallToolResponse, McpError> {
                let prefix = request.name.split('-').next().unwrap_or_default();
                match prefix {
                    $($prefix => self.$field.call_tool(request, context).await,)*
                    _ => Err(McpError::invalid_params("no such tool", None)),
                }
            }
        }
    };
}

endpoint! {
    admin: admin::AdminServiceMcpHandler = "admin_service",
    entity: entity::EntityServiceMcpHandler = "entity_service",
    signal: signal::SignalServiceMcpHandler = "signal_service",
    stream: stream::StreamServiceMcpHandler = "stream_service",
    query: query::QueryServiceMcpHandler = "query_service",
    state: state::StateServiceMcpHandler = "state_service",
    replay: replay::ReplayServiceMcpHandler = "replay_service",
}

impl ServerHandler for Endpoint {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(PROTOCOLS)
    }

    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new("norn", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let mut tools = Self::tools();
        tools.iter_mut().for_each(|t| t.output_schema = None);
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !Self::tools().iter().any(|t| t.name == request.name) {
            let name = request.name;
            return Err(McpError::invalid_params(format!("no tool {name}"), None));
        }
        match self.dispatch(request, context).await {
            Err(e) if e.data.is_some() => {
                let text = json!({ "error": e.data }).to_string();
                Ok(CallToolResult::error(vec![ContentBlock::text(text)]).into())
            }
            done => done,
        }
    }
}
