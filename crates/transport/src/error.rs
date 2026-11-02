//! The transport error model.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Everything binding, accepting or connecting can fail with.
///
/// Callers match on the variant, never on the message: a live socket
/// (another daemon is running) and an insecure directory (fix the
/// permissions) call for different reactions.
#[derive(Debug)]
#[non_exhaustive]
pub enum TransportError {
    /// A Unix socket at this path is accepting connections: another server
    /// owns it, so it is never replaced (SPEC §41).
    LiveSocket(PathBuf),
    /// Something that is not a socket occupies the path. Norn never deletes
    /// a file it did not create.
    NotASocket(PathBuf),
    /// The socket's directory grants access to group or others, so the
    /// socket's own 0600 mode would not be the only gate.
    InsecureDirectory {
        /// The offending directory.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
    /// TCP is a loopback-only test fallback; this address is not loopback.
    NotLoopback(SocketAddr),
    /// This kind of endpoint does not exist on the running OS, e.g. a named
    /// pipe on Linux.
    Unsupported(&'static str),
    /// The connection limits are out of range.
    InvalidLimits(&'static str),
    /// The underlying OS call failed.
    Io(io::Error),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LiveSocket(p) => write!(f, "a live server owns socket {}", p.display()),
            Self::NotASocket(p) => write!(f, "{} exists and is not a socket", p.display()),
            Self::InsecureDirectory { path, mode } => write!(
                f,
                "socket directory {} has mode {mode:o}; it must be owner-only (0700)",
                path.display()
            ),
            Self::NotLoopback(a) => write!(f, "tcp endpoint {a} is not loopback"),
            Self::Unsupported(what) => write!(f, "{what} is not supported on this OS"),
            Self::InvalidLimits(why) => write!(f, "invalid connection limits: {why}"),
            Self::Io(e) => write!(f, "transport I/O: {e}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
