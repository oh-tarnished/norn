//! AIP-193 errors: a `google.rpc.Status` carrying one `ErrorInfo` in the
//! `norn.local` domain (docs/reference/protocol.md, "Error model").

use std::collections::HashMap;

use norn_protocol::PROTOCOL_VERSION;
use norn_protocol::common::ErrorReason;
use norn_protocol::rpc::{ErrorDetails, StatusExt, pb};
use prost::Message;
use tonic::{Code, Status};

/// The `ErrorInfo.domain` of every error Norn returns.
pub const ERROR_DOMAIN: &str = "norn.local";

/// A status with `code`, `message`, and one `ErrorInfo` whose reason is
/// `reason`'s name and whose metadata is `metadata`.
pub(crate) fn error_info<'a>(
    code: Code,
    reason: ErrorReason,
    message: impl Into<String>,
    metadata: impl IntoIterator<Item = (&'a str, String)>,
) -> Status {
    let metadata: HashMap<String, String> = metadata
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    let details = ErrorDetails::with_error_info(reason.as_str_name(), ERROR_DOMAIN, metadata);
    Status::with_error_details(code, message, details)
}

/// `FAILED_PRECONDITION` for a client whose protocol version, `client`, this
/// daemon cannot speak. Metadata gives the server's version.
pub(crate) fn version_unsupported(client: impl std::fmt::Display) -> Status {
    let ours = PROTOCOL_VERSION;
    error_info(
        Code::FailedPrecondition,
        ErrorReason::ProtocolVersionUnsupported,
        format!(
            "client protocol {client} is not supported; this daemon speaks {}.{}",
            ours.major, ours.minor
        ),
        [
            ("server_major", ours.major.to_string()),
            ("server_minor", ours.minor.to_string()),
        ],
    )
}

/// `INVALID_ARGUMENT` with `ERROR_REASON_INVALID_ARGUMENT`, metadata `field`,
/// and a `BadRequest` field violation.
pub(crate) fn bad_field(field: &str, description: &str) -> Status {
    let mut details = ErrorDetails::new();
    details.add_bad_request_violation(field, description);
    let metadata = HashMap::from([("field".to_owned(), field.to_owned())]);
    details.set_error_info(
        ErrorReason::InvalidArgument.as_str_name(),
        ERROR_DOMAIN,
        metadata,
    );
    Status::with_error_details(
        Code::InvalidArgument,
        format!("{field}: {description}"),
        details,
    )
}

/// The `google.rpc.Status` inside `status`, for a response field. Copies.
pub(crate) fn to_rpc(status: &Status) -> pb::Status {
    let mut rpc = pb::Status::decode(status.details()).unwrap_or_default();
    rpc.code = status.code() as i32;
    rpc.message = status.message().to_owned();
    rpc
}
