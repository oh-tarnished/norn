//! Adapts [`Incoming`] to the connection stream tonic serves from.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use norn_transport::{Connection, Incoming, TransportError};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tonic::transport::server::Connected;

/// Pause after a failed accept before the next attempt, so a persistent
/// error (e.g. the process is out of file descriptors) cannot spin a core.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// One admitted connection, as tonic sees it.
///
/// A newtype only because tonic's [`Connected`] trait and the transport's
/// [`Connection`] are both foreign to this crate. Reports no connect info:
/// the transport is deliberately invisible above this layer (SPEC §7).
pub struct IpcStream(Connection);

impl Connected for IpcStream {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl AsyncRead for IpcStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl AsyncWrite for IpcStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, b)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

type Accepting =
    Pin<Box<dyn Future<Output = (Incoming, Result<Connection, TransportError>)> + Send>>;

/// A never-ending stream of admitted connections.
///
/// Owns the [`Incoming`] (moving it into each accept future and back), so
/// dropping the stream releases the endpoint. Accept errors are yielded —
/// tonic logs and continues — and the next accept is delayed by a short
/// backoff.
pub(crate) struct IncomingStream {
    idle: Option<Incoming>,
    accepting: Option<Accepting>,
    backoff: bool,
}

impl IncomingStream {
    pub(crate) fn new(incoming: Incoming) -> Self {
        Self {
            idle: Some(incoming),
            accepting: None,
            backoff: false,
        }
    }
}

impl futures_core::Stream for IncomingStream {
    type Item = Result<IpcStream, TransportError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.accepting.is_none() {
            let Some(mut incoming) = this.idle.take() else {
                return Poll::Ready(None);
            };
            let backoff = std::mem::take(&mut this.backoff);
            this.accepting = Some(Box::pin(async move {
                if backoff {
                    tokio::time::sleep(ACCEPT_BACKOFF).await;
                }
                let result = incoming.accept().await;
                (incoming, result)
            }));
        }
        let Some(accepting) = this.accepting.as_mut() else {
            return Poll::Ready(None);
        };
        let (incoming, result) = std::task::ready!(accepting.as_mut().poll(cx));
        this.accepting = None;
        this.idle = Some(incoming);
        this.backoff = result.is_err();
        Poll::Ready(Some(result.map(IpcStream)))
    }
}
