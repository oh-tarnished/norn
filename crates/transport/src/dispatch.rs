//! From an [`Endpoint`] to a listener or a stream, without the caller
//! naming a transport.

use crate::platform::os;
use crate::{BoxIo, Connector, Endpoint, IoFuture, Listener, TransportError, tcp};

/// Binds `endpoint` with no limits applied; most servers want
/// [`Incoming::bind`](crate::Incoming::bind) instead.
///
/// Unix sockets get the SPEC §41 protections (owner-only directory and
/// socket, stale socket replaced, live socket refused). Named pipes get an
/// owner-only DACL and refuse a name another process serves. TCP must be
/// loopback. Never spawns a task.
pub async fn bind(endpoint: &Endpoint) -> Result<Box<dyn Listener>, TransportError> {
    match endpoint {
        Endpoint::Unix(path) => os::bind_unix(path).await,
        Endpoint::Pipe(name) => os::bind_pipe(name).await,
        Endpoint::Tcp(addr) => tcp::bind(*addr).await,
    }
}

/// Opens one client connection to `endpoint`.
///
/// Fails fast (no retries) except on Windows while every pipe instance is
/// busy, which is retried for up to a second.
pub async fn connect(endpoint: &Endpoint) -> Result<BoxIo, TransportError> {
    match endpoint {
        Endpoint::Unix(path) => os::connect_unix(path).await,
        Endpoint::Pipe(name) => os::connect_pipe(name).await,
        Endpoint::Tcp(addr) => tcp::connect(*addr).await,
    }
}

/// A [`Connector`] for one fixed endpoint.
///
/// `Clone + Send + Sync`; each `connect` call opens an independent stream.
#[derive(Clone, Debug)]
pub struct EndpointConnector {
    endpoint: Endpoint,
}

impl EndpointConnector {
    /// A connector that always dials `endpoint`.
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    /// The endpoint this connector dials.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl Connector for EndpointConnector {
    fn connect(&self) -> IoFuture<'static> {
        let endpoint = self.endpoint.clone();
        Box::pin(async move { connect(&endpoint).await })
    }
}
