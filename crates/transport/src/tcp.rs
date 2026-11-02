//! Loopback TCP: the portable fallback, for tests only.
//!
//! TCP carries no OS-level identity check, so any local user can connect.
//! It exists so the gRPC stack can be exercised identically on every OS;
//! the daemon's default is always a Unix socket or a named pipe. Binding a
//! non-loopback address is refused.

use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};

use crate::{BoxIo, Endpoint, IoFuture, Listener, TransportError};

/// A loopback TCP listener.
pub(crate) struct TcpLoopbackListener {
    listener: TcpListener,
    endpoint: Endpoint,
}

/// Binds `addr`, which must be loopback. Port 0 picks an ephemeral port.
pub(crate) async fn bind(addr: SocketAddr) -> Result<Box<dyn Listener>, TransportError> {
    if !addr.ip().is_loopback() {
        return Err(TransportError::NotLoopback(addr));
    }
    let listener = TcpListener::bind(addr).await?;
    let endpoint = Endpoint::Tcp(listener.local_addr()?);
    Ok(Box::new(TcpLoopbackListener { listener, endpoint }))
}

/// Connects to `addr`, which must be loopback, with Nagle disabled.
pub(crate) async fn connect(addr: SocketAddr) -> Result<BoxIo, TransportError> {
    if !addr.ip().is_loopback() {
        return Err(TransportError::NotLoopback(addr));
    }
    let stream = TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    Ok(Box::new(stream))
}

impl Listener for TcpLoopbackListener {
    fn accept(&mut self) -> IoFuture<'_> {
        Box::pin(async move {
            let (stream, _) = self.listener.accept().await?;
            // gRPC frames are small and latency-bound; Nagle only delays them.
            stream.set_nodelay(true)?;
            let io: BoxIo = Box::new(stream);
            Ok(io)
        })
    }

    fn local_endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
