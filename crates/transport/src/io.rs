//! The transport-neutral byte stream and the listener/connector traits.

use std::future::Future;
use std::pin::Pin;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{Endpoint, TransportError};

/// A bidirectional byte stream from any transport.
///
/// Implemented for every `AsyncRead + AsyncWrite + Send + Unpin + 'static`
/// type, so a Unix socket, a named pipe and a TCP stream are all `Io`.
/// Code above this crate sees only [`BoxIo`] and so never learns which
/// transport it is on (SPEC §7).
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin + 'static {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Io for T {}

/// An owned, type-erased stream. Dropping it closes the connection.
pub type BoxIo = Box<dyn Io>;

/// The future returned by [`Listener::accept`] and [`Connector::connect`].
pub type IoFuture<'a> = Pin<Box<dyn Future<Output = Result<BoxIo, TransportError>> + Send + 'a>>;

/// A bound server endpoint that yields raw connections.
///
/// Implementations live in the platform modules and in `tcp.rs`; obtain one
/// with [`bind`](crate::bind). A listener applies no limits of its own:
/// [`Incoming`](crate::Incoming) wraps it with admission control and idle
/// timeouts.
///
/// # Concurrency
///
/// `accept` takes `&mut self`: one task accepts at a time. Accepted
/// streams are independent and may move to other tasks.
///
/// # Shutdown
///
/// Dropping the listener stops accepting and releases the OS resource
/// (for a Unix socket, removes the socket file if it is still ours).
/// Connections already accepted are unaffected.
pub trait Listener: Send + 'static {
    /// Waits for the next connection. Cancel-safe: dropping the future
    /// before it completes loses no connection; a client that arrived
    /// meanwhile is returned by the next call.
    fn accept(&mut self) -> IoFuture<'_>;

    /// The endpoint actually bound, e.g. with an ephemeral TCP port filled in.
    fn local_endpoint(&self) -> &Endpoint;
}

/// Opens client connections to one endpoint.
///
/// Implemented by [`EndpointConnector`](crate::EndpointConnector). `Sync`
/// so one connector may serve concurrent connection attempts.
pub trait Connector: Send + Sync + 'static {
    /// Opens one connection. The future owns everything it needs.
    fn connect(&self) -> IoFuture<'static>;
}
