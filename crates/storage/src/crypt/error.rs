//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Why encrypted data could not be read or written. Never carries key bytes.

use std::fmt;
use std::path::PathBuf;

use super::KeyId;

/// What is wrong with a key file or key variable. The line is 1-based; no
/// variant holds any part of a key, so the error is safe to log.
///
/// Concurrency: plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    /// No key at all: the file or variable is empty or only comments.
    Empty,
    /// A line is not 64 hexadecimal digits (32 bytes).
    Malformed {
        /// The offending line, counted from 1.
        line: usize,
    },
    /// Two lines hold the same key.
    Duplicate {
        /// The second occurrence, counted from 1.
        line: usize,
    },
    /// More than [`super::MAX_KEYS`] keys.
    TooMany,
    /// The key file is readable by its group or by others (Unix mode bits
    /// other than `0600`/`0400`), or is not a regular file.
    Insecure {
        /// The key file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
    /// The key file could not be read.
    Unreadable {
        /// The key file.
        path: PathBuf,
        /// The OS error, as text.
        reason: String,
    },
}

/// An encryption-at-rest failure (docs/reference/storage.md "Encryption").
///
/// Concurrency: plain data; `Clone` so a reader can report it on every read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncryptionError {
    /// The segment was encrypted under a key the keyring does not hold: a
    /// wrong key file, or an old key removed before `nornctl rekey` finished.
    /// Nothing was changed.
    KeyUnavailable {
        /// The segment.
        path: PathBuf,
        /// The id of the key it needs.
        key: KeyId,
    },
    /// The segment's encryption does not match the configuration: an
    /// encrypted segment with encryption off (`encrypted: true`), or a plain
    /// one with it on. Nothing was changed.
    Mismatch {
        /// The segment or database.
        path: PathBuf,
        /// Whether what was found is encrypted.
        encrypted: bool,
    },
    /// The segment names a cipher or key derivation this build does not know.
    UnsupportedCipher {
        /// The segment.
        path: PathBuf,
        /// The cipher byte, then the KDF byte.
        found: (u8, u8),
    },
    /// The key file or variable is unusable.
    Key(KeyError),
    /// The operating system's random source failed, so no salt could be made
    /// and nothing was written.
    Random,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("no key: expected a line of 64 hex digits"),
            Self::Malformed { line } => write!(f, "line {line} is not 64 hex digits"),
            Self::Duplicate { line } => write!(f, "line {line} repeats an earlier key"),
            Self::TooMany => write!(f, "more than {} keys", super::MAX_KEYS),
            Self::Insecure { path, mode } => write!(
                f,
                "{} has mode {mode:04o}: a key file must be readable by its owner only \
                 (chmod 600)",
                path.display()
            ),
            Self::Unreadable { path, reason } => write!(f, "{}: {reason}", path.display()),
        }
    }
}

impl fmt::Display for EncryptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyUnavailable { path, key } => write!(
                f,
                "{} is encrypted with key {key}, which the configured key file does not \
                 hold: wrong key, or an old key removed before `nornctl rekey` finished",
                path.display()
            ),
            Self::Mismatch {
                path,
                encrypted: true,
            } => write!(
                f,
                "{} is encrypted, but [storage] encryption is off: configure its key",
                path.display()
            ),
            Self::Mismatch { path, .. } => write!(
                f,
                "{} is not encrypted, but [storage] encryption is on: an unencrypted store \
                 cannot be opened encrypted",
                path.display()
            ),
            Self::UnsupportedCipher { path, found } => write!(
                f,
                "{}: cipher {} with key derivation {} is not supported by this build",
                path.display(),
                found.0,
                found.1
            ),
            Self::Key(e) => write!(f, "encryption key: {e}"),
            Self::Random => f.write_str("the operating system's random source failed"),
        }
    }
}

impl std::error::Error for KeyError {}
impl std::error::Error for EncryptionError {}

impl From<KeyError> for EncryptionError {
    fn from(e: KeyError) -> Self {
        Self::Key(e)
    }
}
