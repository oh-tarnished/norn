//! Holds calls until the daemon runs. The listener opens in `Daemon::bind`, which
//! is what claims the endpoint, but a call is answered only once `Daemon::run` has
//! started: a client that connects early waits, as it would in a socket's backlog.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::{Request, Response};
use tokio::sync::watch;
use tonic::Status;
use tonic::body::Body;
use tower::{Layer, Service};

/// Wraps every service in [`Opening`]. Cheap to clone.
#[derive(Debug, Clone)]
pub(crate) struct OpeningLayer(pub(crate) watch::Receiver<bool>);

impl<S> Layer<S> for OpeningLayer {
    type Service = Opening<S>;

    fn layer(&self, inner: S) -> Opening<S> {
        Opening {
            inner,
            open: self.0.clone(),
        }
    }
}

/// A call waits here until the daemon runs; if it never will, it is `UNAVAILABLE`.
/// Concurrency: `Clone`, as concurrent as the service it wraps.
#[derive(Debug, Clone)]
pub(crate) struct Opening<S> {
    inner: S,
    open: watch::Receiver<bool>,
}

impl<S, B> Service<Request<B>> for Opening<S>
where
    S: Service<Request<B>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let clone = self.inner.clone();
        let mut ready = std::mem::replace(&mut self.inner, clone);
        let mut open = self.open.clone();
        Box::pin(async move {
            if open.wait_for(|running| *running).await.is_err() {
                let status = Status::unavailable("the daemon stopped before it served");
                return Ok(status.into_http());
            }
            ready.call(request).await
        })
    }
}
