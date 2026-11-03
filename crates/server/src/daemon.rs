//! The daemon: a runtime whose tasks serve gRPC over local IPC.

mod error;
mod router;
mod serve;
mod tasks;

pub use error::DaemonError;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use norn_ipc::{Endpoint, Server, ServerConfig};
use norn_runtime::ingest::{Ingest, IngestRecovery};
use norn_runtime::maintenance::{CompactionPhase, Maintenance};
use norn_runtime::sessions::Replays;
use norn_runtime::{HealthWatch, Reservation, RunReport, Runtime};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tonic_health::server::HealthReporter;

use self::router::{build_router, open_database};
use crate::config::DaemonConfig;
use crate::health::reflect;

/// Slack the runtime gives the IPC task beyond its own drain bound, so the
/// server finishes aborting connections before the runtime aborts it.
const SHUTDOWN_SLACK: Duration = Duration::from_secs(5);

/// The Norn daemon, bound to its endpoint and ready to run.
///
/// Lifecycle: [`bind`](Daemon::bind) checks the endpoint (so a second daemon
/// fails there, before touching a database), opens the database and runs crash
/// recovery, builds every service and opens the listener, whose calls wait until
/// [`run`](Daemon::run) starts. So a client's first call
/// sees the recovered state. Dropping a bound daemon stops the listener, on a
/// task of the current runtime, and releases the database. `run` serves until
/// cancelled, then drains in-flight calls (bounded by the configured drain
/// timeout), removes the socket, and returns once every task has stopped.
///
/// Concurrency: `Send`; it is consumed by `run`. Take a
/// [`health`](Daemon::health) watch first to observe it while it runs.
pub struct Daemon {
    runtime: Runtime,
    server: serve::Bound,
    endpoint: Endpoint,
    /// Whether calls are answered yet.
    opening: watch::Sender<bool>,
    /// Cancelled when MCP's stdio client leaves.
    ended: CancellationToken,
    reporter: HealthReporter,
    ingest: Arc<Ingest>,
    recovery: IngestRecovery,
    replays: Arc<Replays>,
    maintenance: Arc<Maintenance>,
    /// The tasks calls spawn, waited for before ingestion closes.
    calls: TaskTracker,
    /// The half of `ipc_bytes` promised to flow-control windows.
    _windows: Reservation,
}

impl fmt::Debug for Daemon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Daemon")
            .field("endpoint", &self.endpoint)
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
        // Claims and releases: a live daemon, a squatter or an insecure directory
        // fails here with its own error, before a database is touched.
        drop(Server::bind(&config.endpoint(), ServerConfig::default()).await?);

        let database = open_database(&config, &runtime).await?;
        let (ingest, recovery) = (Arc::clone(&database.ingest), database.recovery.clone());
        let (opening, open) = watch::channel(false);
        let ended = CancellationToken::new();
        let wiring = serve::Wiring {
            open,
            ended: ended.clone(),
        };
        let built = build_router(&config, &runtime, database, wiring).await?;
        let (server, endpoint) = serve::Bound::start(built.server, &config.endpoint()).await?;
        Ok(Self {
            runtime,
            server,
            endpoint,
            opening,
            ended,
            reporter: built.reporter,
            ingest,
            recovery,
            replays: built.replays,
            maintenance: built.maintenance,
            calls: built.calls,
            _windows: built.windows,
        })
    }

    /// Calls `probe` at each phase of every compaction, on the worker's thread. For
    /// tests that must stop a real process at an exact point; a `norn` never sets
    /// one. Before [`run`](Daemon::run).
    pub fn compaction_probe(&self, probe: Box<dyn Fn(CompactionPhase) + Send + Sync>) {
        self.maintenance.set_probe(probe);
    }

    /// What startup recovery found. Also published as `Server.recovery`.
    #[must_use]
    pub fn recovery(&self) -> &IngestRecovery {
        &self.recovery
    }

    /// The endpoint actually bound (with any ephemeral port resolved).
    #[must_use]
    pub fn local_endpoint(&self) -> &Endpoint {
        &self.endpoint
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
            endpoint: _,
            opening,
            ended,
            reporter,
            ingest,
            recovery: _,
            replays,
            maintenance,
            calls,
            _windows,
        } = self;
        // Nothing has been answered until now; every task below is added first.
        let watch = runtime.health();
        let stop = cancel.clone();
        runtime.add_task("mcp-stdio", move |token| {
            tasks::stop_when(ended, stop, token)
        })?;
        runtime.add_task("health", move |token| reflect(watch, reporter, token))?;
        let draining = Arc::clone(&ingest);
        runtime.add_task("ingest", move |token| tasks::drain_ingest(draining, token))?;
        // Expires idle sessions; on shutdown ends them all and joins their tasks.
        runtime.add_task("replay", move |token| async move {
            replays.run(token).await;
            Ok(())
        })?;
        // Cancelled by dropping the guard, so a panicking worker cannot hang
        // shutdown behind it.
        let quiet = CancellationToken::new();
        let done = quiet.clone().drop_guard();
        runtime.add_task("maintenance", move |token| async move {
            let _done = done;
            maintenance.run(token).await;
            Ok(())
        })?;
        runtime.add_task("ipc", move |token| async move {
            let _ = opening.send(true);
            token.cancelled().await;
            tasks::stop_serving(server, calls, quiet, ingest).await
        })?;
        Ok(runtime.run(cancel).await?)
    }
}
