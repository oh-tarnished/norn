//! The daemon's server: runtime-rs's [`HybridServer`], configured to Norn's rules.
//!
//! gRPC is always on, on the local endpoint: `without_grpc_listener` is never
//! called, and `enable_http` never set, so a default daemon opens no TCP port. MCP
//! is the agent services `[mcp]` names. Every call, from a socket or from MCP
//! through the loopback, passes the opening gate, the version gate and metered
//! decoding, as layers around every service. Health is the daemon's own, which goes
//! `NOT_SERVING` for the drain; reflection stays off (`Production`).

use std::sync::Arc;
use std::time::Duration;

use norn_core::Config;
use norn_ipc::Endpoint;
use norn_protocol::FILE_DESCRIPTOR_SET;
use norn_protocol::admin::admin_service_server::AdminServiceServer;
use norn_protocol::entity::entity_service_server::EntityServiceServer;
use norn_protocol::google::longrunning::operations_server::OperationsServer;
use norn_protocol::query::query_service_server::QueryServiceServer;
use norn_protocol::replay::replay_service_server::ReplayServiceServer;
use norn_protocol::signal::signal_service_server::SignalServiceServer;
use norn_protocol::state::state_service_server::StateServiceServer;
use norn_protocol::stream::stream_service_server::StreamServiceServer;
use norn_protocol::telemetry::telemetry_service_server::TelemetryServiceServer;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tonic_health::pb::health_server::{Health, HealthServer};
use tpp_grpc::options::{self, GrpcLimits, Options, Overflow, ServerEnvironment};
use tpp_grpc::{BannerOutput, HybridServer, ListenAddr};

use super::DaemonError;
use crate::admin::Admin;
use crate::catalog::Catalog;
use crate::config::DaemonConfig;
use crate::info::stream_window;
use crate::mcp::dial::{Dial, NoRest};
use crate::mcp::transport;
use crate::metered::MeterLayer;
use crate::opening::OpeningLayer;
use crate::operations::OperationsService;
use crate::query::Queries;
use crate::replay::ReplaySessions;
use crate::state::States;
use crate::telemetry::Telemetry;
use crate::version_gate::VersionGateLayer;

/// The name the server reports, and the prefix of the runtime's environment
/// overrides (`<NAME>_HYBRID_GRPC_PORT`). Its hyphen is deliberate: no shell exports
/// a variable named `NORN-DAEMON_…`, so only the configuration places a listener.
const NAME: &str = "norn-daemon";

/// Concurrent calls per connection (HTTP/2 `MAX_CONCURRENT_STREAMS`). Picked,
/// not measured; stage 15 revisits it. Excess calls wait on the client side. It
/// divides the connection window, so more calls means slower large messages.
const MAX_STREAMS_PER_CONNECTION: u32 = 16;

/// A connection silent this long is closed (docs/reference/protocol.md).
const IDLE: Duration = Duration::from_secs(600);

/// Slack past the drain bound before the server cuts off the rest.
const STOP_SLACK: Duration = Duration::from_secs(2);

/// What connects the server to the daemon's lifecycle.
pub(super) struct Wiring {
    /// Turns true when calls may be answered.
    pub(super) open: watch::Receiver<bool>,
    /// Cancelled when MCP's stdio client leaves.
    pub(super) ended: CancellationToken,
}

/// Every gRPC service, wrapped.
#[allow(missing_docs, reason = "each is its own name")]
pub(super) struct Registered<H: Health> {
    pub(super) health: HealthServer<H>,
    pub(super) admin: AdminServiceServer<Admin>,
    pub(super) entities: EntityServiceServer<Catalog>,
    pub(super) signals: SignalServiceServer<Catalog>,
    pub(super) streams: StreamServiceServer<Catalog>,
    pub(super) telemetry: TelemetryServiceServer<Telemetry>,
    pub(super) queries: QueryServiceServer<Queries>,
    pub(super) replays: ReplayServiceServer<ReplaySessions>,
    pub(super) states: StateServiceServer<States>,
    pub(super) operations: OperationsServer<OperationsService>,
}

fn grpc_endpoint(endpoint: &Endpoint) -> options::Endpoint {
    match endpoint {
        Endpoint::Unix(path) => options::Endpoint::unix(path),
        Endpoint::Pipe(name) => options::Endpoint::pipe(name),
        Endpoint::Tcp(a) => options::Endpoint::new(a.ip().to_string(), a.port()),
        other => options::Endpoint::new(other.to_string(), 0),
    }
}

fn limits(engine: &Config, drain: Duration, window: u32) -> GrpcLimits {
    GrpcLimits::default()
        .max_connections(engine.limits().max_clients)
        .overflow(Overflow::Queue)
        .idle_timeout(IDLE)
        .drain_timeout(drain)
        .max_concurrent_streams(MAX_STREAMS_PER_CONNECTION)
        .initial_connection_window_size(window)
        .initial_stream_window_size(stream_window(window, MAX_STREAMS_PER_CONNECTION))
}

/// The server, configured and not yet started.
pub(super) fn build<H: Health>(
    config: &DaemonConfig,
    wiring: Wiring,
    s: Registered<H>,
    meter: MeterLayer,
    window: u32,
) -> HybridServer {
    let dial = Dial::new();
    let mcp = transport::services(config.mcp_settings(), dial.clone(), &wiring.ended);
    let drain = config.drain();
    let options = Options {
        service_name: NAME.into(),
        description: "Norn, a temporal data engine".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        grpc: grpc_endpoint(&config.endpoint()),
        enable_mcp: !mcp.is_empty(),
        environment: ServerEnvironment::Production,
        ..Options::default()
    };
    let server = HybridServer::new(options)
        .with_banner(BannerOutput::Off)
        .with_reflection(FILE_DESCRIPTOR_SET)
        .with_grpc_limits(limits(config.engine(), drain, window))
        .with_shutdown_timeout(drain.saturating_add(STOP_SLACK))
        .with_grpc_layer(OpeningLayer(wiring.open))
        .with_grpc_layer(VersionGateLayer)
        .with_grpc_layer(meter)
        .with_service(s.health)
        .with_service(s.admin)
        .with_service(s.entities)
        .with_service(s.signals)
        .with_service(s.streams)
        .with_service(s.telemetry)
        .with_service(s.queries)
        .with_service(s.replays)
        .with_service(s.states)
        .with_service(s.operations)
        .with_gateway(move |loopback| {
            dial.publish(loopback);
            NoRest
        });
    mcp.into_iter()
        .fold(server, HybridServer::with_agent_service_dyn)
}

/// A started server. Dropped unstopped, it stops itself on a task, so a bound daemon
/// dropped without running releases its endpoint. Correct from any task.
pub(super) struct Bound(Option<Arc<HybridServer>>);

impl Bound {
    /// Starts `server`, returning it and the endpoint it actually bound.
    pub(super) async fn start(
        mut server: HybridServer,
        wanted: &Endpoint,
    ) -> Result<(Self, Endpoint), DaemonError> {
        server.start().await.map_err(DaemonError::Server)?;
        let bound = match server.grpc_listen_addr() {
            Some(ListenAddr::Tcp(address)) => Endpoint::Tcp(address),
            _ => wanted.clone(),
        };
        Ok((Self(Some(Arc::new(server))), bound))
    }

    /// Drains and stops.
    pub(super) async fn stop(mut self) -> Result<(), tpp_grpc::Error> {
        match self.0.take() {
            Some(server) => server.stop().await,
            None => Ok(()),
        }
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        if let (Some(server), Ok(handle)) = (self.0.take(), tokio::runtime::Handle::try_current()) {
            handle.spawn(async move { server.stop().await });
        }
    }
}
