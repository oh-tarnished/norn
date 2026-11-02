//! The daemon's configuration: engine `Config` plus serving and logging.
//!
//! Precedence, lowest first: `norn_core::Config::default()` and the
//! defaults below, then the TOML file (`--config`), then command-line
//! flags. See docs/guides/configuration.md for the file format.

mod args;
mod engine;
mod error;
mod file;

use std::path::PathBuf;
use std::time::Duration;

use norn_core::Config;
use norn_ipc::Endpoint;

pub use args::{Invocation, USAGE, parse_args};
pub use error::ConfigError;
pub use file::MAX_FILE_BYTES;

/// How long in-flight calls may finish after shutdown is requested, before
/// their connections are aborted (norn-ipc's drain bound). Default 10 s.
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Validated daemon configuration.
///
/// `Default` is a working daemon: the core's default engine `Config`, the
/// platform's default endpoint (`Endpoint::platform_default()`), a 10 s
/// drain and [`LogConfig::default`]. Concurrency: plain data, `Send + Sync`.
#[derive(Debug, Clone, Default)]
pub struct DaemonConfig {
    pub(crate) engine: Config,
    pub(crate) listen: Option<Endpoint>,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) drain_timeout: Option<Duration>,
    pub(crate) log: LogConfig,
}

impl DaemonConfig {
    /// The defaults with `engine` as the engine configuration.
    #[must_use]
    pub fn new(engine: Config) -> Self {
        Self {
            engine,
            ..Self::default()
        }
    }

    /// Serves on `endpoint` instead of the platform default.
    #[must_use]
    pub fn listen(mut self, endpoint: Endpoint) -> Self {
        self.listen = Some(endpoint);
        self
    }

    /// Keeps the database in `dir` instead of the per-user default.
    #[must_use]
    pub fn data_dir(mut self, dir: PathBuf) -> Self {
        self.data_dir = Some(dir);
        self
    }

    /// Sets the drain bound; see [`DEFAULT_DRAIN_TIMEOUT`].
    #[must_use]
    pub fn drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = Some(timeout);
        self
    }

    /// Replaces the logging settings.
    #[must_use]
    pub fn log(mut self, log: LogConfig) -> Self {
        self.log = log;
        self
    }

    /// The engine configuration: memory budgets and limits.
    #[must_use]
    pub fn engine(&self) -> &Config {
        &self.engine
    }

    /// The endpoint to serve on.
    #[must_use]
    pub fn endpoint(&self) -> Endpoint {
        self.listen
            .clone()
            .unwrap_or_else(Endpoint::platform_default)
    }

    /// The database directory: as set, or `$XDG_DATA_HOME/norn`,
    /// `~/.local/share/norn`, or `%LOCALAPPDATA%\\norn`, whichever is defined
    /// first, or `norn-data` under the working directory.
    #[must_use]
    pub fn database(&self) -> PathBuf {
        let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
        self.data_dir
            .clone()
            .or_else(|| var("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("norn")))
            .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/share/norn")))
            .or_else(|| var("LOCALAPPDATA").map(|d| PathBuf::from(d).join("norn")))
            .unwrap_or_else(|| PathBuf::from("norn-data"))
    }

    /// The drain bound.
    #[must_use]
    pub fn drain(&self) -> Duration {
        self.drain_timeout.unwrap_or(DEFAULT_DRAIN_TIMEOUT)
    }

    /// The logging settings.
    #[must_use]
    pub fn logging(&self) -> &LogConfig {
        &self.log
    }
}

/// Where and how the daemon logs. Logs always go to stderr; a supervisor
/// (systemd, launchd, a service manager) owns files and rotation.
///
/// `Default`: level `info`, text format, records cut at 4 KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogConfig {
    /// The most verbose level written.
    pub level: LogLevel,
    /// Human-readable text or JSON lines.
    pub format: LogFormat,
    /// The longest record written, in bytes; longer ones are cut and marked
    /// `[truncated]`. Bounds what one event can cost (SPEC §34).
    pub max_record_bytes: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Text,
            max_record_bytes: 4096,
        }
    }
}

/// A log level, most severe first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs, reason = "the variant names are the documentation")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// The log record format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// One human-readable line per event.
    Text,
    /// One JSON object per line, for log collectors.
    Json,
}

impl std::str::FromStr for LogLevel {
    type Err = ConfigError;
    fn from_str(s: &str) -> Result<Self, ConfigError> {
        Ok(match s {
            "error" => Self::Error,
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => return Err(ConfigError::invalid("log.level", s)),
        })
    }
}

impl std::str::FromStr for LogFormat {
    type Err = ConfigError;
    fn from_str(s: &str) -> Result<Self, ConfigError> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(ConfigError::invalid("log.format", s)),
        }
    }
}
