//! Linux and macOS: Unix domain sockets. Named pipes are Windows-only.

mod socket_file;

use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use crate::{BoxIo, Endpoint, IoFuture, Listener, TransportError};
use socket_file::SocketGuard;

/// File name of the default socket inside its per-user directory.
const SOCKET_NAME: &str = "norn.sock";

/// A bound Unix socket that removes its file when dropped.
///
/// Field order matters: the listener closes before the guard unlinks.
struct UnixSocketListener {
    listener: UnixListener,
    _guard: SocketGuard,
    endpoint: Endpoint,
}

/// Binds a Unix socket at `path` with the SPEC §41 protections: owner-only
/// directory, stale socket replaced, live socket or foreign file refused,
/// socket mode 0600, removal on drop if still ours.
pub(crate) async fn bind_unix(path: &Path) -> Result<Box<dyn Listener>, TransportError> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        socket_file::prepare_directory(dir)?;
    }
    socket_file::clear_stale(path).await?;
    let listener = UnixListener::bind(path)?;
    let guard = SocketGuard::new(path)?;
    socket_file::restrict_socket(path)?;
    Ok(Box::new(UnixSocketListener {
        listener,
        _guard: guard,
        endpoint: Endpoint::Unix(path.to_path_buf()),
    }))
}

/// Connects to the Unix socket at `path`.
pub(crate) async fn connect_unix(path: &Path) -> Result<BoxIo, TransportError> {
    let stream = UnixStream::connect(path).await?;
    Ok(Box::new(stream))
}

/// Named pipes do not exist here.
pub(crate) async fn bind_pipe(_name: &str) -> Result<Box<dyn Listener>, TransportError> {
    Err(TransportError::Unsupported("a named pipe"))
}

/// Named pipes do not exist here.
pub(crate) async fn connect_pipe(_name: &str) -> Result<BoxIo, TransportError> {
    Err(TransportError::Unsupported("a named pipe"))
}

/// `$XDG_RUNTIME_DIR/norn/norn.sock`, else `$TMPDIR/norn/norn.sock`, else
/// `/tmp/norn-<user>/norn.sock`.
///
/// `XDG_RUNTIME_DIR` is per-user and 0700 by specification; macOS's
/// `TMPDIR` is per-user as well. `/tmp` is shared, hence the user suffix;
/// a directory another user pre-created there is refused at bind time
/// rather than used.
pub(crate) fn default_endpoint() -> Endpoint {
    let base = ["XDG_RUNTIME_DIR", "TMPDIR"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|p| p.is_absolute());
    let dir = match base {
        Some(base) => base.join("norn"),
        None => {
            let user = std::env::var("USER").unwrap_or_else(|_| "user".to_owned());
            PathBuf::from(format!("/tmp/norn-{user}"))
        }
    };
    Endpoint::Unix(dir.join(SOCKET_NAME))
}

impl Listener for UnixSocketListener {
    fn accept(&mut self) -> IoFuture<'_> {
        Box::pin(async move {
            let (stream, _) = self.listener.accept().await?;
            let io: BoxIo = Box::new(stream);
            Ok(io)
        })
    }

    fn local_endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
