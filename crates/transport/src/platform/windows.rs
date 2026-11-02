//! Windows: named pipes. Unix sockets are not offered here: tokio has no
//! AF_UNIX support on Windows, and named pipes are the native local IPC.

mod security;

use std::io;
use std::path::Path;
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

use crate::{BoxIo, Endpoint, IoFuture, Listener, TransportError};

/// How many times a client retries a pipe whose instances are all busy.
const BUSY_RETRIES: u32 = 50;
/// Pause between busy retries; 50 × 20 ms bounds the wait at one second.
const BUSY_BACKOFF: Duration = Duration::from_millis(20);

/// A named pipe server. One instance is always waiting for a client; when
/// it connects, a fresh instance replaces it before the connected one is
/// handed out, so there is never a moment with no instance listening.
struct PipeListener {
    name: String,
    descriptor: security::OwnerOnly,
    next: NamedPipeServer,
    endpoint: Endpoint,
}

/// Creates one pipe instance: local clients only, owner-only DACL, and for
/// the first instance, a guarantee that no other process already owns the
/// name (so a squatter is refused, the analogue of a live Unix socket).
fn create(
    name: &str,
    descriptor: &security::OwnerOnly,
    first: bool,
) -> io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .access_inbound(true)
        .access_outbound(true);
    descriptor.create(&options, name)
}

/// Unix sockets are not offered on Windows.
pub(crate) async fn bind_unix(_path: &Path) -> Result<Box<dyn Listener>, TransportError> {
    Err(TransportError::Unsupported("a Unix socket"))
}

/// Unix sockets are not offered on Windows.
pub(crate) async fn connect_unix(_path: &Path) -> Result<BoxIo, TransportError> {
    Err(TransportError::Unsupported("a Unix socket"))
}

/// Binds the named pipe `name` with an owner-only security descriptor.
///
/// Fails with `PermissionDenied` (from `first_pipe_instance`) when another
/// process already serves the name.
pub(crate) async fn bind_pipe(name: &str) -> Result<Box<dyn Listener>, TransportError> {
    let descriptor = security::OwnerOnly::for_current_user()?;
    let next = create(name, &descriptor, true)?;
    Ok(Box::new(PipeListener {
        name: name.to_owned(),
        descriptor,
        next,
        endpoint: Endpoint::Pipe(name.to_owned()),
    }))
}

/// Opens the named pipe `name`, retrying for up to a second while every
/// server instance is busy.
pub(crate) async fn connect_pipe(name: &str) -> Result<BoxIo, TransportError> {
    let mut attempts = 0;
    loop {
        match ClientOptions::new().open(name) {
            Ok(client) => return Ok(Box::new(client)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                attempts += 1;
                if attempts >= BUSY_RETRIES {
                    return Err(e.into());
                }
                tokio::time::sleep(BUSY_BACKOFF).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// `\\.\pipe\norn-<user>`, or `\\.\pipe\norn` when `USERNAME` is unset or
/// contains characters outside `[A-Za-z0-9_.-]`.
pub(crate) fn default_endpoint() -> Endpoint {
    let user = std::env::var("USERNAME").ok().filter(|u| {
        !u.is_empty()
            && u.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    });
    match user {
        Some(user) => Endpoint::Pipe(format!(r"\\.\pipe\norn-{user}")),
        None => Endpoint::Pipe(r"\\.\pipe\norn".to_owned()),
    }
}

impl Listener for PipeListener {
    fn accept(&mut self) -> IoFuture<'_> {
        Box::pin(async move {
            // Cancel-safe: if dropped here, `next` stays in place and a
            // client that connected meanwhile is returned by the next call.
            self.next.connect().await?;
            let fresh = create(&self.name, &self.descriptor, false)?;
            let connected = std::mem::replace(&mut self.next, fresh);
            let io: BoxIo = Box::new(connected);
            Ok(io)
        })
    }

    fn local_endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
