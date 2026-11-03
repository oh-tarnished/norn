//! Small runtime tasks of the daemon's run.

use std::sync::Arc;

use norn_runtime::TaskError;
use norn_runtime::ingest::Ingest;
use tokio_util::sync::CancellationToken;

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
