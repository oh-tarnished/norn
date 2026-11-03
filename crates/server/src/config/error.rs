//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Why configuration could not be loaded.

use std::fmt;
use std::path::PathBuf;

/// A configuration that could not be read, parsed or validated. The daemon
/// refuses to start on any of these rather than guess.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The command line was malformed; the message says how, and the usage
    /// text should be shown.
    Usage(String),
    /// The configuration file could not be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is larger than [`MAX_FILE_BYTES`](crate::config::MAX_FILE_BYTES).
    TooLarge {
        /// The file.
        path: PathBuf,
    },
    /// The file is not valid TOML for the schema, including an unknown key
    /// (a typo is refused, not ignored).
    Parse {
        /// The file.
        path: PathBuf,
        /// The parser's description, with line and column.
        message: String,
    },
    /// A value is out of its domain (an unknown log level, a malformed
    /// endpoint, a zero limit).
    Invalid {
        /// The setting, such as `log.level` or `listen`.
        setting: &'static str,
        /// The offending value or the reason.
        detail: String,
    },
    /// The engine configuration failed norn-core's validation.
    Engine(norn_core::Error),
}

impl ConfigError {
    pub(crate) fn invalid(setting: &'static str, detail: impl fmt::Display) -> Self {
        Self::Invalid {
            setting,
            detail: detail.to_string(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(m) => f.write_str(m),
            Self::Read { path, source } => write!(f, "reading {}: {source}", path.display()),
            Self::TooLarge { path } => write!(f, "{} is too large", path.display()),
            Self::Parse { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Invalid { setting, detail } => write!(f, "invalid {setting}: {detail}"),
            Self::Engine(e) => write!(f, "engine configuration: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Engine(e) => Some(e),
            _ => None,
        }
    }
}
