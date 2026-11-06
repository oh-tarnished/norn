//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A running fake daemon on a real socket: the telemetry fake, beside stubs that
//! refuse batch creates, or a registry that makes them as Norn does.

use crate::fake::{Fake, Log};
use crate::registry::Registry;
use crate::stubs::Stubs;
use norn_ipc::Endpoint;
use norn_protocol::admin::admin_service_server::AdminServiceServer;
use norn_protocol::entity::entity_service_server::EntityServiceServer;
use norn_protocol::signal::signal_service_server::SignalServiceServer;
use norn_protocol::stream::stream_service_server::StreamServiceServer;
use norn_protocol::telemetry::telemetry_service_server::TelemetryServiceServer;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;

/// A running fake; stops when asked, as a daemon that went away.
pub struct Daemon {
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Daemon {
    /// Serves `log` on `endpoint`, creating nothing in batches; it answers once
    /// this returns.
    pub async fn start(endpoint: &Endpoint, log: Log) -> Self {
        let stop = CancellationToken::new();
        let router = Server::builder()
            .add_service(AdminServiceServer::new(Stubs))
            .add_service(EntityServiceServer::new(Stubs))
            .add_service(SignalServiceServer::new(Stubs))
            .add_service(StreamServiceServer::new(Stubs))
            .add_service(TelemetryServiceServer::new(Fake(log, stop.clone())));
        Self::serve(endpoint, router, stop).await
    }

    /// As [`Daemon::start`], with `registry` making entities and signals, and
    /// `log` told what it made.
    pub async fn with_registry(endpoint: &Endpoint, log: Log, registry: Registry) -> Self {
        log.lock().unwrap().made = Some(registry.made());
        let stop = CancellationToken::new();
        let router = Server::builder()
            .add_service(AdminServiceServer::new(registry.clone()))
            .add_service(EntityServiceServer::new(registry.clone()))
            .add_service(SignalServiceServer::new(registry))
            .add_service(StreamServiceServer::new(Stubs))
            .add_service(TelemetryServiceServer::new(Fake(log, stop.clone())));
        Self::serve(endpoint, router, stop).await
    }

    async fn serve(
        endpoint: &Endpoint,
        router: tonic::transport::server::Router,
        stop: CancellationToken,
    ) -> Self {
        let server = norn_ipc::Server::bind(endpoint, norn_ipc::ServerConfig::default())
            .await
            .unwrap();
        let token = stop.clone();
        let task = tokio::spawn(async move {
            let _ = server.serve(router, token).await;
        });
        Self { stop, task }
    }

    /// Goes away, as `SIGKILL` would look from the far side of the socket.
    pub async fn stop(self) {
        self.stop.cancel();
        self.task.abort();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}
