//! Serving a tonic router over a local endpoint, with bounded shutdown.

use std::time::Duration;

use bytes::Bytes;
use http::{Request, Response};
use norn_transport::{Endpoint, Incoming, Limits};
use tokio_util::sync::CancellationToken;
use tonic::body::Body;
use tonic::service::Routes;
use tonic::transport::server::Router;
use tower::{Layer, Service};

use crate::IpcError;
use crate::stream::IncomingStream;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// After the drain bound, how long aborted connections get to notice.
/// They fail on their next poll, which is immediate; one second is slack.
const ABORT_GRACE: Duration = Duration::from_secs(1);

/// Optional server settings. `Default`: [`Limits::default`] and a 10 s drain.
#[derive(Clone, Copy, Debug)]
pub struct ServerConfig {
    limits: Limits,
    drain: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            drain: Duration::from_secs(10),
        }
    }
}

impl ServerConfig {
    /// Sets the connection limits.
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets how long in-flight calls may run after shutdown is requested
    /// before their connections are aborted.
    #[must_use]
    pub fn drain_timeout(mut self, drain: Duration) -> Self {
        self.drain = drain;
        self
    }
}

/// How a server stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Connections still open when the drain bound expired, and therefore
    /// aborted. Zero means every connection closed gracefully.
    pub aborted: usize,
}

/// A gRPC server bound to a local endpoint and not yet serving.
///
/// [`bind`](Server::bind) claims the endpoint (so a second daemon fails
/// there, not later); [`serve`](Server::serve) runs until shutdown. Nothing
/// is spawned before `serve`, so a bound server can be inspected (e.g. its
/// ephemeral port) or dropped, which releases the endpoint.
pub struct Server {
    incoming: Incoming,
    drain: Duration,
}

impl Server {
    /// Binds `endpoint` with `config`'s limits.
    pub async fn bind(endpoint: &Endpoint, config: ServerConfig) -> Result<Self, IpcError> {
        let incoming = Incoming::bind(endpoint, config.limits).await?;
        Ok(Self {
            incoming,
            drain: config.drain,
        })
    }

    /// The endpoint actually bound.
    pub fn local_endpoint(&self) -> &Endpoint {
        self.incoming.local_endpoint()
    }

    /// Serves `router` until `shutdown` is cancelled, then drains.
    ///
    /// # Shutdown
    ///
    /// On cancellation the endpoint is released at once (no new clients),
    /// open HTTP/2 connections receive GOAWAY, and in-flight calls run to
    /// completion. If connections remain after the drain timeout, they are
    /// aborted — their next read or write fails — and the count is reported.
    /// Returns only once every connection task tonic spawned has ended, so
    /// nothing outlives the call, except handler futures for calls whose
    /// connection was aborted, which tonic runs as separate tasks and which
    /// finish on their own.
    ///
    /// # Errors
    ///
    /// [`IpcError::Grpc`] if tonic fails; [`IpcError::DrainStuck`] if
    /// aborted connections still had not closed after a further second.
    pub async fn serve<L, ResBody>(
        self,
        router: Router<L>,
        shutdown: CancellationToken,
    ) -> Result<ShutdownReport, IpcError>
    where
        L: Layer<Routes>,
        L::Service: Service<Request<Body>, Response = Response<ResBody>> + Clone + Send + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send,
        <L::Service as Service<Request<Body>>>::Error: Into<BoxError> + Send,
        ResBody: http_body::Body<Data = Bytes> + Send + 'static,
        ResBody::Error: Into<BoxError>,
    {
        let tracker = self.incoming.tracker();
        let abort = self.incoming.abort_handle();
        let signal = shutdown.clone().cancelled_owned();
        let stream = IncomingStream::new(self.incoming);
        let server = router.serve_with_incoming_shutdown(stream, signal);
        tokio::pin!(server);
        let deadline = async {
            shutdown.cancelled().await;
            tokio::time::sleep(self.drain).await;
        };
        tokio::select! {
            result = &mut server => {
                result?;
                return Ok(ShutdownReport { aborted: 0 });
            }
            () = deadline => {}
        }
        let aborted = tracker.active();
        abort.cancel();
        match tokio::time::timeout(ABORT_GRACE, &mut server).await {
            Ok(result) => {
                result?;
                Ok(ShutdownReport { aborted })
            }
            Err(_) => Err(IpcError::DrainStuck {
                open: tracker.active(),
            }),
        }
    }
}

/// Binds `endpoint` with [`ServerConfig::default`] and serves `router`
/// until `shutdown` is cancelled. See [`Server::serve`] for the drain.
pub async fn serve<L, ResBody>(
    endpoint: &Endpoint,
    router: Router<L>,
    shutdown: CancellationToken,
) -> Result<ShutdownReport, IpcError>
where
    L: Layer<Routes>,
    L::Service: Service<Request<Body>, Response = Response<ResBody>> + Clone + Send + 'static,
    <L::Service as Service<Request<Body>>>::Future: Send,
    <L::Service as Service<Request<Body>>>::Error: Into<BoxError> + Send,
    ResBody: http_body::Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<BoxError>,
{
    let server = Server::bind(endpoint, ServerConfig::default()).await?;
    server.serve(router, shutdown).await
}
