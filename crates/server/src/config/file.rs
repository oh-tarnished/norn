//! The TOML configuration file: every key optional, unknown keys refused.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::engine::{Count, Memory, Meta, Storage, engine, set};
use super::{ConfigError, DaemonConfig, LogConfig};

/// The largest configuration file read. A configuration is a few hundred
/// bytes; the bound keeps a wrong path (a log, a device) from being slurped.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The smallest `log.max_record_bytes`: room for a timestamp, a level and
/// the start of a message.
const MIN_RECORD_BYTES: usize = 128;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    listen: Option<String>,
    data_dir: Option<String>,
    drain_timeout_ms: Option<u64>,
    log: Option<Log>,
    memory: Option<Memory>,
    limits: Option<Count>,
    metadata: Option<Meta>,
    storage: Option<Storage>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Log {
    level: Option<String>,
    format: Option<String>,
    max_record_bytes: Option<usize>,
}

impl DaemonConfig {
    /// Reads `path` (at most [`MAX_FILE_BYTES`]) and parses it with
    /// [`from_toml`](DaemonConfig::from_toml). Blocking file I/O: call it
    /// before starting the async runtime, as `norn` does.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Read`], [`ConfigError::TooLarge`], or any error of
    /// [`from_toml`](DaemonConfig::from_toml).
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let read = |source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        };
        let file = std::fs::File::open(path).map_err(read)?;
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(read)?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(ConfigError::TooLarge {
                path: path.to_path_buf(),
            });
        }
        Self::from_toml(&text).map_err(|e| match e {
            ConfigError::Parse { message, .. } => ConfigError::Parse {
                path: path.to_path_buf(),
                message,
            },
            other => other,
        })
    }

    /// Parses a configuration from TOML text over the defaults. Pure.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Parse`] for malformed TOML or an unknown key,
    /// [`ConfigError::Invalid`] for a bad value, and
    /// [`ConfigError::Engine`] when norn-core rejects the result.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let file: File = toml::from_str(text).map_err(|e| ConfigError::Parse {
            path: "<toml>".into(),
            message: e.to_string(),
        })?;
        let mut config = Self::default();
        if let Some(listen) = file.listen {
            config.listen = Some(
                listen
                    .parse()
                    .map_err(|e| ConfigError::invalid("listen", e))?,
            );
        }
        config.data_dir = file.data_dir.map(Into::into);
        config.drain_timeout = file.drain_timeout_ms.map(Duration::from_millis);
        let l = file.log.unwrap_or_default();
        let mut log = LogConfig::default();
        if let Some(level) = l.level {
            log.level = level.parse()?;
        }
        if let Some(format) = l.format {
            log.format = format.parse()?;
        }
        set(&mut log.max_record_bytes, l.max_record_bytes);
        if log.max_record_bytes < MIN_RECORD_BYTES {
            let detail = format!("below the minimum of {MIN_RECORD_BYTES}");
            return Err(ConfigError::invalid("log.max_record_bytes", detail));
        }
        config.log = log;
        config.engine = engine(file.memory, file.limits, file.metadata, file.storage)?;
        Ok(config)
    }
}
