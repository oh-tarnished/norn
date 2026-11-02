//! Why a `nornctl` command failed.

use std::fmt;

use norn_ipc::{Endpoint, IpcError};
use norn_protocol::rpc::StatusExt;

/// A failed command. Displayed to the user as one line; match on the
/// variant to tell an absent daemon from a refusal.
#[derive(Debug)]
#[non_exhaustive]
pub enum CliError {
    /// No daemon answered at the endpoint (not running, wrong endpoint, or
    /// not this user's).
    Connect {
        /// Where we looked.
        endpoint: Endpoint,
        /// Why it failed.
        source: IpcError,
    },
    /// The daemon answered a call with an error status.
    Call {
        /// The method, such as `AdminService.NegotiateProtocol`.
        method: &'static str,
        /// What it returned; its `ErrorInfo` reason is shown when present.
        status: tonic::Status,
    },
    /// The daemon answered with a response missing a required field.
    Malformed {
        /// The method.
        method: &'static str,
        /// The missing field.
        field: &'static str,
    },
    /// The words after a resource command make no sense; a usage message.
    Usage(String),
    /// The daemon refused a batch; its reason, for the user.
    Refused(String),
    /// Writing the output failed (a closed pipe, a full disk).
    Output(std::io::Error),
    /// An export or import failed; it says where in the file.
    Adapter(norn_adapter::AdapterError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { endpoint, source } => {
                write!(f, "cannot reach the daemon at {endpoint}: {source}")
            }
            Self::Call { method, status } => {
                write!(f, "{method}: {:?}: {}", status.code(), status.message())?;
                if let Some(info) = status.get_details_error_info() {
                    write!(f, " ({} {})", info.domain, info.reason)?;
                    let mut metadata: Vec<_> = info.metadata.iter().collect();
                    metadata.sort();
                    for (k, v) in metadata {
                        write!(f, " {k}={v}")?;
                    }
                }
                Ok(())
            }
            Self::Malformed { method, field } => write!(f, "{method}: response lacks {field}"),
            Self::Usage(text) | Self::Refused(text) => f.write_str(text),
            Self::Output(e) => write!(f, "writing output: {e}"),
            Self::Adapter(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect { source, .. } => Some(source),
            Self::Call { status, .. } => Some(status),
            Self::Output(e) => Some(e),
            Self::Adapter(e) => Some(e),
            Self::Malformed { .. } | Self::Usage(_) | Self::Refused(_) => None,
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Output(e)
    }
}
