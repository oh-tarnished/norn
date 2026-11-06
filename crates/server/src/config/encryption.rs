//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `[storage] encryption`, and where its keys come from. Never from the TOML:
//! `deny_unknown_fields` refuses any key written there.

use std::path::PathBuf;

use norn_runtime::storage::{EncryptionError, Keyring};

use super::ConfigError;

/// Where the master keys are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeySource {
    /// A file of hexadecimal keys, owner-only (`0600` or `0400`).
    File(PathBuf),
    /// An environment variable holding the same text. Visible to anything that
    /// can read this process's environment; prefer a file.
    Env(String),
}

impl KeySource {
    /// Reads the keys: current first.
    ///
    /// # Errors
    /// [`EncryptionError::Key`] for an unusable file or variable. No error holds
    /// any part of a key.
    pub fn load(&self) -> Result<Keyring, EncryptionError> {
        match self {
            Self::File(path) => Keyring::from_file(path).map_err(EncryptionError::Key),
            Self::Env(name) => Keyring::from_env(name).map_err(EncryptionError::Key),
        }
    }
}

/// `encryption`, `key_file` and `key_env` as checked settings: `None` for off.
pub(super) fn source(
    encryption: Option<&str>,
    file: Option<PathBuf>,
    env: Option<String>,
) -> Result<Option<KeySource>, ConfigError> {
    let bad = |detail| ConfigError::invalid("storage.encryption", detail);
    match (encryption.unwrap_or("off"), file, env) {
        ("off", None, None) => Ok(None),
        ("off", _, _) => Err(bad("key_file and key_env need encryption = \"on\"")),
        ("on", Some(file), None) => Ok(Some(KeySource::File(file))),
        ("on", None, Some(env)) if !env.is_empty() => Ok(Some(KeySource::Env(env))),
        ("on", _, _) => Err(bad("\"on\" needs exactly one of key_file and key_env")),
        _ => Err(bad("must be \"off\" or \"on\"")),
    }
}
