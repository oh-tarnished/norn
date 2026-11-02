//! SIGINT and SIGTERM.

use std::future::Future;
use std::io;

use tokio::signal::unix::{SignalKind, signal};

pub(super) fn register() -> io::Result<impl Future<Output = ()> + Send> {
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => tracing::info!(signal = "SIGINT", "shutdown requested"),
            _ = terminate.recv() => tracing::info!(signal = "SIGTERM", "shutdown requested"),
        }
    })
}
