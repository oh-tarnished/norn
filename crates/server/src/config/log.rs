//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Where and how the daemon logs.

use super::ConfigError;

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
