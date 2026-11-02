//! A tool's failure, in terms a model can act on.
//!
//! Every failure is a tool result with `isError` set, never a protocol error: per
//! the MCP specification, an error the model can correct belongs where the model
//! reads it. Its text is one JSON object, so a caller branches on `reason` and
//! never on `message`.

use std::collections::BTreeMap;

use norn_protocol::rpc::StatusExt;
use serde_json::{Value, json};
use tonic::Code;

/// Why a tool call failed.
///
/// Concurrency: plain owned data, `Send + Sync`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    /// Which failure: a daemon `ERROR_REASON_*` name, or one of this server's
    /// `MCP_*` and `DAEMON_*` names. Stable; match on this.
    pub reason: String,
    /// What happened, for the model to read.
    pub message: String,
    /// What to try next.
    pub hint: Option<String>,
    /// Whether the same call may succeed later unchanged.
    pub retryable: bool,
    /// The daemon's `ErrorInfo` metadata, such as `chunk` and `min_time`.
    pub metadata: BTreeMap<String, String>,
}

impl ToolError {
    pub(crate) fn new(reason: &str, message: impl Into<String>) -> Self {
        Self {
            reason: reason.to_owned(),
            message: message.into(),
            hint: None,
            retryable: false,
            metadata: BTreeMap::new(),
        }
    }

    /// Bad arguments: `MCP_INVALID_ARGUMENT`, naming the argument.
    pub(crate) fn invalid(argument: &str, why: impl std::fmt::Display) -> Self {
        Self::new("MCP_INVALID_ARGUMENT", format!("{argument}: {why}"))
    }

    /// A continuation that cannot be used: `MCP_INVALID_CONTINUATION`.
    pub(crate) fn continuation(why: &str) -> Self {
        let mut e = Self::new("MCP_INVALID_CONTINUATION", why);
        e.hint = Some("Repeat the call without `continuation` to start again.".to_owned());
        e
    }

    /// A bound of this server was passed: `MCP_LIMIT_EXCEEDED`, naming it.
    pub(crate) fn limit(limit: &str, value: usize, hint: &str) -> Self {
        let mut e = Self::new("MCP_LIMIT_EXCEEDED", format!("{limit} is {value} here"));
        e.hint = Some(hint.to_owned());
        e.metadata.insert("limit".to_owned(), limit.to_owned());
        e
    }

    /// The whole call passed its deadline: `MCP_DEADLINE_EXCEEDED`.
    pub(crate) fn deadline() -> Self {
        let mut e = Self::new("MCP_DEADLINE_EXCEEDED", "the call passed its deadline");
        e.hint = Some("Narrow the interval or the selection, or lower `limit`.".to_owned());
        e.retryable = true;
        e
    }

    /// The failure of daemon method `method`.
    pub(crate) fn daemon(method: &str, status: &tonic::Status) -> Self {
        let info = status.get_details_error_info();
        let gone = is_gone(status) && info.is_none();
        let reason = match &info {
            Some(info) => info.reason.clone(),
            None if gone => "DAEMON_UNAVAILABLE".to_owned(),
            None => format!("GRPC_{:?}", status.code()).to_uppercase(),
        };
        let mut e = Self::new(&reason, format!("{method}: {}", status.message()));
        e.metadata = info
            .map(|i| i.metadata.into_iter().collect())
            .unwrap_or_default();
        e.retryable = gone || is_transient(status.code());
        e.hint = crate::hints::hint(&reason, &e.metadata);
        e
    }

    /// The object a tool result's text holds.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "error": {
                "reason": self.reason,
                "message": self.message,
                "hint": self.hint,
                "retryable": self.retryable,
                "metadata": self.metadata,
            }
        })
    }
}

/// Whether the connection to the daemon failed, as opposed to the daemon answering.
/// A connection torn down mid-stream reaches tonic as `Unknown` or `Cancelled`
/// with no `ErrorInfo`, which every answer from the daemon itself carries.
fn is_gone(status: &tonic::Status) -> bool {
    matches!(
        status.code(),
        Code::Unavailable | Code::Unknown | Code::Cancelled
    )
}

fn is_transient(code: Code) -> bool {
    matches!(
        code,
        Code::Unavailable | Code::ResourceExhausted | Code::Aborted
    )
}
