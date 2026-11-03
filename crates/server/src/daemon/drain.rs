//! Counts every gRPC service clone and request future until its resources drop.
//!
//! A transport finishing its drain does not join every HTTP/2 request task.
//! Those tasks can still hold a service (and its registry lock) for a few polls.
//! The daemon's existing call tracker must wait for these owners too.

use std::task::{Context, Poll};

use tokio_util::task::TaskTracker;
use tokio_util::task::task_tracker::{TaskTrackerToken, TrackedFuture};
use tower::{Layer, Service};

#[cfg(test)]
mod tests;

/// Tracks services without counting the layer retained by the server itself.
#[derive(Clone)]
pub(super) struct DrainLayer(pub(super) TaskTracker);

impl<S> Layer<S> for DrainLayer {
    type Service = Drain<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Drain {
            inner,
            token: self.0.token(),
        }
    }
}

/// Field order releases the service's resources before notifying shutdown.
#[derive(Clone)]
pub(super) struct Drain<S> {
    inner: S,
    token: TaskTrackerToken,
}

impl<S, R> Service<R> for Drain<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = TrackedFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        self.token
            .task_tracker()
            .track_future(self.inner.call(request))
    }
}
