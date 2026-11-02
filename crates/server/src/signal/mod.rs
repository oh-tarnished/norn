//! Operating-system shutdown requests, behind one function per platform.
//!
//! The only `cfg` in this crate selects the module; both export
//! `register() -> io::Result<impl Future<Output = ()>>`.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
use windows as platform;

use std::future::Future;
use std::io;

/// Registers for the platform's shutdown requests and returns a future that
/// completes on the first one: SIGINT or SIGTERM on Unix, Ctrl-C or
/// Ctrl-Break on Windows.
///
/// Registration happens now, so a request arriving before the future is
/// first polled is not lost. Needs a tokio runtime with signals enabled.
///
/// # Errors
///
/// The OS refused the registration; the daemon should not start, since it
/// could then only be stopped by force.
pub fn shutdown_requested() -> io::Result<impl Future<Output = ()> + Send> {
    platform::register()
}
