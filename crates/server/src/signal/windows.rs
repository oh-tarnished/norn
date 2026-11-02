//! Ctrl-C and Ctrl-Break from the console.
//!
//! A Windows service would use the Service Control Manager's stop request
//! instead; running `norn` as a service is not supported yet.

use std::future::Future;
use std::io;

use tokio::signal::windows::{ctrl_break, ctrl_c};

pub(super) fn register() -> io::Result<impl Future<Output = ()> + Send> {
    let mut interrupt = ctrl_c()?;
    let mut brk = ctrl_break()?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => tracing::info!(signal = "Ctrl-C", "shutdown requested"),
            _ = brk.recv() => tracing::info!(signal = "Ctrl-Break", "shutdown requested"),
        }
    })
}
