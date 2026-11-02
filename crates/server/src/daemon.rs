//! The daemon: a runtime whose tasks serve gRPC over local IPC.

mod error;
mod router;

pub use error::DaemonError;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use norn_ipc::{Endpoint, Limits, Server, ServerConfig};
use norn_runtime::ingest::{Ingest, IngestRecovery};
use norn_runtime::sessions::Replays;
use norn_runtime::{HealthWatch, Reservation, RunReport, Runtime};
use tokio_util::sync::CancellationToken;
use tonic_health::server::HealthReporter;

use self::router::{build_router, open_database};
use crate::config::DaemonConfig;
use crate::health::reflect;

/// Slack the runtime gives the IPC task beyond its own drain bound, so the
/// server finishes aborting connections before the runtime aborts it.
const SHUTDOWN_SLACK: Duration = Duration::from_secs(5);

/// The Norn daemon, bound to its endpoint and ready to run.
///
/// Lifecycle: [`bind`](Daemon::bind) claims the endpoint (so a second
/// daemon fails there), opens the database and runs crash recovery, so a
/// client's first call sees the recovered state, and builds every service, but
/// spawns no lasting task; dropping a bound daemon releases the endpoint and
/// the database. [`run`](Daemon::run)
/// serves until cancelled, then drains in-flight calls (bounded by the
/// configured drain timeout), removes the socket, and returns once every
/// task has stopped.
///
/// Concurrency: `Send`; it is consumed by `run`. Take a
/// [`health`](Daemon::health) watch first to observe it while it runs.
pub struct Daemon {
    runtime: Runtime,
    server: Server,
    router: router::Services,
    reporter: HealthReporter,
    ingest: Arc<Ingest>,
    recovery: IngestRecovery,
    replays: Arc<Replays>,
    /// The half of `ipc_bytes` promised to flow-control windows.
    _windows: Reservation,
}

impl fmt::Debug for Daemon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Daemon")
            .field("endpoint", self.server.local_endpoint())
            .field("health", &self.runtime.health().get())
            .finish_non_exhaustive()
    }
}

impl Daemon {
    /// Validates `config`, binds its endpoint and builds the services.
    ///
    /// # Errors
    ///
    /// [`DaemonError::Runtime`] if the engine configuration cannot be run
    /// on this platform; [`DaemonError::Ipc`] if the endpoint cannot be
    /// claimed (a live daemon already holds it, an insecure directory...);
    /// [`DaemonError::Metadata`] if the registry cannot be opened, as when
    /// another daemon holds the database; [`DaemonError::Storage`] if recovery
    /// cannot assess a stream.
    pub async fn bind(config: DaemonConfig) -> Result<Self, DaemonError> {
        let drain = config.drain();
        let runtime =
            Runtime::new(*config.engine())?.shutdown_timeout(drain.saturating_add(SHUTDOWN_SLACK));
        let limits = Limits::default().max_connections(config.engine().limits().max_clients);
        let ipc = ServerConfig::default().limits(limits).drain_timeout(drain);
        let server = Server::bind(&config.endpoint(), ipc).await?;

        let database = open_database(&config, &runtime).await?;
        let (ingest, recovery) = (Arc::clone(&database.ingest), database.recovery.clone());
        let (router, windows, reporter, replays) =
            build_router(&config, &runtime, database).await?;
        Ok(Self {
            runtime,
            server,
            router,
            reporter,
            ingest,
            recovery,
            replays,
            _windows: windows,
        })
    }

    /// What startup recovery found. Also published as `Server.recovery`.
    #[must_use]
    pub fn recovery(&self) -> &IngestRecovery {
        &self.recovery
    }

    /// The endpoint actually bound (with any ephemeral port resolved).
    #[must_use]
    pub fn local_endpoint(&self) -> &Endpoint {
        self.server.local_endpoint()
    }

    /// A watch on the daemon's lifecycle; valid after `run` consumes it.
    #[must_use]
    pub fn health(&self) -> HealthWatch {
        self.runtime.health()
    }

    /// Serves until `cancel` is cancelled, then drains and stops.
    ///
    /// # Errors
    ///
    /// [`DaemonError::Runtime`] when a task fails fatally (the IPC server
    /// erroring, or its drain getting stuck), after every task has stopped.
    pub async fn run(self, cancel: CancellationToken) -> Result<RunReport, DaemonError> {
        let Self {
            runtime,
            server,
            router,
            reporter,
            ingest,
            recovery: _,
            replays,
            _windows,
        } = self;
        let watch = runtime.health();
        runtime.add_task("health", move |token| reflect(watch, reporter, token))?;
        let draining = Arc::clone(&ingest);
        runtime.add_task("ingest", move |token| async move {
            token.cancelled().await;
            draining.begin_drain();
            draining.live().close();
            Ok(())
        })?;
        // Expires idle sessions; on shutdown ends them all and joins their tasks.
        runtime.add_task("replay", move |token| async move {
            replays.run(token).await;
            Ok(())
        })?;
        runtime.add_task("ipc", move |token| async move {
            let served = server.serve(router, token).await;
            // After the last call: everything admitted is committed, whether
            // serving ended cleanly or not.
            tokio::task::spawn_blocking(move || ingest.close()).await?;
            let report = served?;
            tracing::info!(aborted = report.aborted, "ipc drained");
            Ok(())
        })?;
        Ok(runtime.run(cancel).await?)
    }
}
