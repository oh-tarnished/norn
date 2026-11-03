//! Small runtime tasks of the daemon's run.

use std::sync::Arc;

use norn_runtime::TaskError;
use norn_runtime::ingest::Ingest;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::serve::Bound;

/// A runtime task: when MCP's stdio client leaves (`ended`), stops the daemon
/// (`stop`), since the client is what it was running for. Ends with `token`.
pub(super) async fn stop_when(
    ended: CancellationToken,
    stop: CancellationToken,
    token: CancellationToken,
) -> Result<(), TaskError> {
    tokio::select! {
        () = token.cancelled() => {}
        () = ended.cancelled() => stop.cancel(),
    }
    Ok(())
}

/// A runtime task: on shutdown, stops ingestion from admitting batches and ends
/// live subscriptions, so the drain is of calls already in flight.
pub(super) async fn drain_ingest(
    ingest: Arc<Ingest>,
    token: CancellationToken,
) -> Result<(), TaskError> {
    token.cancelled().await;
    ingest.begin_drain();
    ingest.live().close();
    Ok(())
}

/// The IPC task's shutdown: stops the server, waits for every call's task,
/// then for maintenance (`quiet`), then closes ingestion.
pub(super) async fn stop_serving(
    server: Bound,
    calls: TaskTracker,
    quiet: CancellationToken,
    ingest: Arc<Ingest>,
) -> Result<(), TaskError> {
    let served = server.stop().await;
    // A call's task outlives its connection by a few polls: an append still
    // answering admitted batches holds the registry, and its LOCK, until it
    // ends. Waited for before ingestion closes, so none is lost, and before
    // `run` returns, so a restart finds the database released.
    calls.close();
    calls.wait().await;
    // Files stop moving before the clean markers list them.
    quiet.cancelled().await;
    // After the last call: everything admitted is committed, whether serving
    // ended cleanly or not.
    tokio::task::spawn_blocking(move || ingest.close()).await?;
    served?;
    tracing::info!("ipc drained");
    Ok(())
}
