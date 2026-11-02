//! Norn's local IPC transports (SPEC §5–7, §41).
//!
//! **Owns:** moving bytes between a local client and the daemon. An
//! [`Endpoint`] names where (`unix:`, `pipe:`, `tcp:`); [`bind`] and
//! [`connect`] turn it into a [`Listener`] or a [`BoxIo`] stream;
//! [`Incoming`] adds connection limits, backpressure and idle timeouts.
//!
//! **Does not:** speak gRPC (that is `norn-ipc`), know any Norn type, or
//! frame messages. It depends on no other Norn crate.
//!
//! **Transports:** Unix domain sockets on Linux and macOS, named pipes on
//! Windows, loopback TCP everywhere as a test-only fallback. Platform code
//! lives only in `platform/{unix,windows}.rs`; nothing else carries a `cfg`.
//!
//! **Security (SPEC §41):** a Unix socket is created 0600 in a 0700
//! directory; a stale socket is replaced, a live one or a non-socket file
//! refused; the file is removed on drop only if it is still ours. A named
//! pipe gets a DACL granting only the current user and refuses remote
//! clients. TCP refuses non-loopback addresses.
//!
//! **Unsafe:** none on Unix. On Windows, exactly one module,
//! `platform/windows/security.rs`, builds the pipe's security descriptor
//! through Win32 FFI; its header explains why.
//!
//! ```no_run
//! # async fn demo() -> Result<(), norn_transport::TransportError> {
//! use norn_transport::{Endpoint, Incoming, Limits};
//!
//! let endpoint = Endpoint::platform_default();
//! let mut incoming = Incoming::bind(&endpoint, Limits::default()).await?;
//! let connection = incoming.accept().await?; // AsyncRead + AsyncWrite
//! # drop(connection);
//! # Ok(()) }
//! ```

mod connection;
mod dispatch;
mod endpoint;
mod error;
mod incoming;
mod io;
mod limits;
mod platform;
mod tcp;

pub use connection::Connection;
pub use dispatch::{EndpointConnector, bind, connect};
pub use endpoint::{Endpoint, ParseEndpointError};
pub use error::TransportError;
pub use incoming::{ConnectionTracker, Incoming};
pub use io::{BoxIo, Connector, Io, IoFuture, Listener};
pub use limits::{Limits, MAX_CONNECTIONS_CEILING, Overflow};
