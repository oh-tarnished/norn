//! The in-process path to the gRPC services, published by the running server.
//!
//! An MCP call is a gRPC call in every way that matters: it goes through
//! [`Loopback`], the server's routes behind the same layers a socket call passes
//! (the version gate, metered decoding, the opening gate), and nothing is dialled.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tonic::Status;
use tpp_grpc::{Loopback, RestGateway};

/// How long a call waits for a starting server to publish its loopback. MCP's
/// stdio answers from the moment the runtime mounts it, just before the routes
/// it calls are final.
const STARTING: Duration = Duration::from_secs(5);

/// Where the loopback is published. Cheap to clone; every clone sees the same
/// loopback, from any task.
#[derive(Clone)]
pub(crate) struct Dial(Arc<watch::Sender<Option<Loopback>>>);

impl Dial {
    pub(crate) fn new() -> Self {
        Self(Arc::new(watch::Sender::new(None)))
    }

    /// Publishes `loopback`, replacing any published before.
    pub(crate) fn publish(&self, loopback: Loopback) {
        self.0.send_replace(Some(loopback));
    }

    /// The published loopback, waiting a moment for one being started.
    ///
    /// # Errors
    /// `UNAVAILABLE` when none is published within 5 s.
    pub(crate) async fn get(&self) -> Result<Loopback, Status> {
        let mut published = self.0.subscribe();
        let ready = published.wait_for(Option::is_some);
        match tokio::time::timeout(STARTING, ready).await {
            Ok(Ok(loopback)) => loopback
                .clone()
                .ok_or_else(|| Status::internal("a published loopback vanished")),
            _ => Err(Status::unavailable(
                "the gRPC services are not being served",
            )),
        }
    }
}

/// A REST gateway claiming no route: `with_gateway` is how a server hands out its
/// loopback, and Norn serves no REST (docs/reference/protocol.md).
pub(crate) struct NoRest;

impl RestGateway for NoRest {
    fn claims(&self, _: &http::Method, _: &str) -> bool {
        false
    }

    fn serve(
        &self,
        _: axum::extract::Request,
    ) -> std::pin::Pin<Box<dyn Future<Output = axum::response::Response> + Send>> {
        Box::pin(async {
            let mut response = axum::response::Response::default();
            *response.status_mut() = http::StatusCode::NOT_FOUND;
            response
        })
    }
}
