//! The IPC error model.

use std::fmt;

use norn_transport::TransportError;

/// Everything serving or connecting can fail with. Match on the variant.
#[derive(Debug)]
#[non_exhaustive]
pub enum IpcError {
    /// Binding or dialling the endpoint failed; the inner error says why
    /// (a live socket, an insecure directory, an unsupported transport…).
    Transport(TransportError),
    /// tonic failed to serve or to establish the HTTP/2 channel.
    Grpc(tonic::transport::Error),
    /// Connections were aborted after the drain bound, yet the server had
    /// still not stopped within the abort grace period. Indicates a
    /// connection task that ignores I/O errors; the server future was
    /// dropped and `open` connections may linger until their tasks end.
    DrainStuck {
        /// Connections still open when the server gave up waiting.
        open: usize,
    },
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "ipc transport: {e}"),
            Self::Grpc(e) => write!(f, "ipc grpc: {e}"),
            Self::DrainStuck { open } => {
                write!(f, "{open} connections still open after abort")
            }
        }
    }
}

impl std::error::Error for IpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(e) => Some(e),
            Self::Grpc(e) => Some(e),
            Self::DrainStuck { .. } => None,
        }
    }
}

impl From<TransportError> for IpcError {
    fn from(e: TransportError) -> Self {
        Self::Transport(e)
    }
}

impl From<tonic::transport::Error> for IpcError {
    fn from(e: tonic::transport::Error) -> Self {
        Self::Grpc(e)
    }
}
