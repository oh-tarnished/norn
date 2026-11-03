//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How a failed call reads to a model: the daemon's reason, its metadata, and whether
//! trying again can help. The endpoint makes each an `isError` result.

use norn_protocol::rpc::StatusExt;
use rmcp::ErrorData as McpError;
use serde_json::{Value, json};
use tonic::{Code, Status};

/// A failure as a model reads it: the daemon's reason, its metadata, and whether
/// trying again can help. [`super::endpoint`] makes it an `isError` result.
pub(super) fn failure(reason: &str, message: &str, retryable: bool, metadata: Value) -> McpError {
    let data = json!({
        "reason": reason, "message": message, "retryable": retryable, "metadata": metadata,
    });
    McpError::internal_error(message.to_owned(), Some(data))
}

pub(super) fn refused(status: &Status) -> McpError {
    let info = status.get_details_error_info();
    let reason = info.as_ref().map_or_else(
        || format!("GRPC_{:?}", status.code()).to_uppercase(),
        |i| i.reason.clone(),
    );
    let metadata = info.map_or_else(|| json!({}), |i| json!(i.metadata));
    let again = matches!(
        status.code(),
        Code::Unavailable | Code::ResourceExhausted | Code::Aborted
    );
    failure(&reason, status.message(), again, metadata)
}

/// A method with no tool. The endpoint never lists or dispatches it either.
pub(super) fn no_tool() -> McpError {
    McpError::invalid_params("this method has no tool: MCP is read-only", None)
}
