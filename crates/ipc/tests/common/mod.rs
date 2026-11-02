//! Shared fixtures: a tiny gRPC service (the standard health protocol, via
//! tonic-health) and helpers to run it over an endpoint.
#![allow(dead_code, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use norn_ipc::{Endpoint, IpcError, Server, ServerConfig, ShutdownReport};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::codegen::tokio_stream;
use tonic::{Request, Response, Status};
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_server::{Health, HealthServer};
use tonic_health::pb::{HealthCheckRequest, HealthCheckResponse};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A Unix socket endpoint in a fresh directory short enough for `sun_path`.
pub fn unix_endpoint() -> Endpoint {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ni{}-{n}", std::process::id()));
    Endpoint::Unix(dir.join("n.sock"))
}

/// The directory of a Unix endpoint, for cleanup.
pub fn unix_dir(endpoint: &Endpoint) -> Option<PathBuf> {
    match endpoint {
        Endpoint::Unix(p) => p.parent().map(PathBuf::from),
        _ => None,
    }
}

/// An ephemeral loopback TCP endpoint.
pub fn tcp_endpoint() -> Endpoint {
    "tcp:127.0.0.1:0".parse().unwrap()
}

/// A health service whose `Check` takes `delay` before answering SERVING.
pub struct SlowHealth {
    pub delay: Duration,
}

#[tonic::async_trait]
impl Health for SlowHealth {
    async fn check(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        tokio::time::sleep(self.delay).await;
        Ok(Response::new(HealthCheckResponse {
            status: ServingStatus::Serving as i32,
        }))
    }

    type WatchStream = tokio_stream::Pending<Result<HealthCheckResponse, Status>>;

    async fn watch(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        Ok(Response::new(tokio_stream::pending()))
    }
}

/// A running test server.
pub struct Running {
    pub endpoint: Endpoint,
    pub shutdown: CancellationToken,
    pub task: JoinHandle<Result<ShutdownReport, IpcError>>,
}

/// Binds `endpoint` with `config` and serves a [`SlowHealth`] with `delay`.
pub async fn start(endpoint: &Endpoint, config: ServerConfig, delay: Duration) -> Running {
    let server = Server::bind(endpoint, config).await.unwrap();
    let endpoint = server.local_endpoint().clone();
    let router =
        tonic::transport::Server::builder().add_service(HealthServer::new(SlowHealth { delay }));
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(server.serve(router, shutdown.clone()));
    Running {
        endpoint,
        shutdown,
        task,
    }
}

/// One `Check` call over `channel`.
pub async fn check(channel: tonic::transport::Channel) -> Result<i32, Status> {
    let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
    let reply = client.check(HealthCheckRequest::default()).await?;
    Ok(reply.into_inner().status)
}
