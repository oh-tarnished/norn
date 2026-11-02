//! The one way to the daemon: gRPC clients over a lazy channel, each call stamped
//! with this client's protocol version and bounded by the tool call's deadline.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norn_ipc::Endpoint;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::state::state_service_client::StateServiceClient;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::{PROTOCOL_VERSION, VERSION_METADATA_KEY};
use tokio::time::{Instant, timeout_at};
use tonic::codegen::InterceptedService;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use crate::error::ToolError;

type Stamp = fn(Request<()>) -> Result<Request<()>, Status>;
pub(crate) type Rpc = InterceptedService<Channel, Stamp>;

/// Sends `norn-protocol-version` on every call, so an incompatible daemon fails the
/// call clearly: the server's job is to outlive a daemon being replaced, so a version
/// checked once at startup would prove nothing about the daemon answering now.
fn stamp(mut request: Request<()>) -> Result<Request<()>, Status> {
    let version = format!("{}.{}", PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor);
    let value = MetadataValue::try_from(version).map_err(|e| Status::internal(e.to_string()))?;
    request.metadata_mut().insert(VERSION_METADATA_KEY, value);
    Ok(request)
}

/// A handle on the daemon. Connecting is lazy and retried by each call, so a daemon
/// that is down when this starts, or restarts while it runs, is a tool error and
/// then not one, never a failure of the MCP server.
///
/// Concurrency: `Clone`, and clones share one HTTP/2 connection, so concurrent tool
/// calls multiplex. Holds no data.
#[derive(Debug, Clone)]
pub struct Daemon {
    channel: Channel,
    timeout: Duration,
}

impl Daemon {
    /// A handle on `endpoint`, whose tool calls each get `timeout` in all. Does no I/O.
    #[must_use]
    pub fn new(endpoint: &Endpoint, timeout: Duration) -> Self {
        Self {
            channel: norn_ipc::connect_lazy(endpoint),
            timeout,
        }
    }

    /// One tool call: its deadline, and what `now` means throughout it.
    pub(crate) fn call(&self) -> Call {
        let now = SystemTime::now().duration_since(UNIX_EPOCH);
        Call {
            daemon: self.clone(),
            deadline: Instant::now() + self.timeout,
            now: now.map_or(0, |d| d.as_nanos() as i128),
        }
    }
}

/// One tool call against the daemon.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    daemon: Daemon,
    /// When the call must have answered.
    pub(crate) deadline: Instant,
    /// The wall clock when it began, nanoseconds since the epoch.
    pub(crate) now: i128,
}

macro_rules! clients {
    ($($name:ident: $client:ident,)*) => {
        impl Call {
            $(pub(crate) fn $name(&self) -> $client<Rpc> {
                $client::with_interceptor(self.daemon.channel.clone(), stamp as Stamp)
            })*
        }
    };
}

clients! {
    entities: EntityServiceClient,
    signals: SignalServiceClient,
    query: QueryServiceClient,
    state: StateServiceClient,
    replays: ReplayServiceClient,
    telemetry: TelemetryServiceClient,
}

impl Call {
    /// Awaits one RPC within the deadline, mapping its failure.
    pub(crate) async fn rpc<T>(
        &self,
        method: &str,
        call: impl Future<Output = Result<Response<T>, Status>>,
    ) -> Result<T, ToolError> {
        match timeout_at(self.deadline, call).await {
            Ok(Ok(reply)) => Ok(reply.into_inner()),
            Ok(Err(status)) => Err(ToolError::daemon(method, &status)),
            Err(_) => Err(ToolError::deadline()),
        }
    }
}
