//! Endpoint addresses: where a Norn daemon listens and a client connects.

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use crate::platform;

/// The local prefix every accepted named pipe name must start with.
/// `\\server\pipe\...` names a remote pipe, which Norn never uses.
const LOCAL_PIPE_PREFIX: &str = r"\\.\pipe\";

/// A local IPC address.
///
/// The textual form is `<scheme>:<address>`, and [`Display`](fmt::Display)
/// and [`FromStr`] round-trip:
///
/// | Form | Meaning |
/// | --- | --- |
/// | `unix:/run/user/1000/norn/norn.sock` | Unix domain socket (Linux, macOS); absolute path |
/// | `pipe:\\.\pipe\norn` | Windows named pipe; local names only |
/// | `tcp:127.0.0.1:7171` | Loopback TCP, a fallback for tests; never a non-loopback address |
///
/// Parsing is pure: it touches no file system and works for every scheme on
/// every OS, so a configuration file can be validated anywhere. Whether the
/// running OS supports the scheme is decided when binding or connecting.
///
/// `Endpoint` is plain data: `Send`, `Sync`, freely cloned.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Endpoint {
    /// A Unix domain socket at an absolute path.
    Unix(PathBuf),
    /// A Windows named pipe, e.g. `\\.\pipe\norn`.
    Pipe(String),
    /// A loopback TCP address. Port 0 binds an ephemeral port; read the real
    /// one back from the bound listener.
    Tcp(SocketAddr),
}

impl Endpoint {
    /// The endpoint a daemon uses when none is configured.
    ///
    /// On Unix: `$XDG_RUNTIME_DIR/norn/norn.sock`, else `$TMPDIR/norn/norn.sock`,
    /// else `/tmp/norn-<user>/norn.sock`. Each of these leaf directories is
    /// created owner-only on bind. On Windows: `\\.\pipe\norn-<user>`, so two
    /// users on one machine never contend for a name.
    ///
    /// Reads environment variables; performs no other I/O.
    pub fn platform_default() -> Self {
        platform::default_endpoint()
    }

    /// The scheme, as written before the colon: `unix`, `pipe` or `tcp`.
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Unix(_) => "unix",
            Self::Pipe(_) => "pipe",
            Self::Tcp(_) => "tcp",
        }
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unix(p) => write!(f, "unix:{}", p.display()),
            Self::Pipe(name) => write!(f, "pipe:{name}"),
            Self::Tcp(addr) => write!(f, "tcp:{addr}"),
        }
    }
}

/// Why an endpoint string was rejected. Each variant carries the input.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseEndpointError {
    /// No `unix:`, `pipe:` or `tcp:` prefix.
    UnknownScheme(String),
    /// A `unix:` path that is empty or relative.
    RelativePath(String),
    /// A `pipe:` name outside `\\.\pipe\`, or with nothing after it.
    BadPipeName(String),
    /// A `tcp:` address that does not parse as `ip:port`.
    BadSocketAddr(String),
    /// A `tcp:` address that is not loopback.
    NotLoopback(String),
}

impl fmt::Display for ParseEndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownScheme(s) => write!(f, "{s:?}: expected unix:, pipe: or tcp:"),
            Self::RelativePath(s) => write!(f, "{s:?}: unix socket path must be absolute"),
            Self::BadPipeName(s) => write!(f, r"{s:?}: pipe name must start with \\.\pipe\"),
            Self::BadSocketAddr(s) => write!(f, "{s:?}: expected ip:port"),
            Self::NotLoopback(s) => write!(f, "{s:?}: tcp is loopback-only"),
        }
    }
}

impl std::error::Error for ParseEndpointError {}

impl FromStr for Endpoint {
    type Err = ParseEndpointError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(path) = s.strip_prefix("unix:") {
            let path = PathBuf::from(path);
            // `/x` is absolute on Unix but not on Windows; accept either
            // spelling so configuration validates the same everywhere.
            if path.is_absolute() || path.starts_with("/") {
                return Ok(Self::Unix(path));
            }
            return Err(ParseEndpointError::RelativePath(s.to_owned()));
        }
        if let Some(name) = s.strip_prefix("pipe:") {
            let has_prefix = name.len() > LOCAL_PIPE_PREFIX.len()
                && name
                    .get(..LOCAL_PIPE_PREFIX.len())
                    .is_some_and(|p| p.eq_ignore_ascii_case(LOCAL_PIPE_PREFIX));
            if has_prefix {
                return Ok(Self::Pipe(name.to_owned()));
            }
            return Err(ParseEndpointError::BadPipeName(s.to_owned()));
        }
        if let Some(addr) = s.strip_prefix("tcp:") {
            let addr: SocketAddr = addr
                .parse()
                .map_err(|_| ParseEndpointError::BadSocketAddr(s.to_owned()))?;
            if !addr.ip().is_loopback() {
                return Err(ParseEndpointError::NotLoopback(s.to_owned()));
            }
            return Ok(Self::Tcp(addr));
        }
        Err(ParseEndpointError::UnknownScheme(s.to_owned()))
    }
}
