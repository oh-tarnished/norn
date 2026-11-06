//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Opt-in encryption at rest: AEAD per chunk body (docs/reference/storage.md
//! "Encryption" has the byte tables and the threat model).
//!
//! # Design
//!
//! - **Cipher**: ChaCha20-Poly1305 (RFC 8439), RustCrypto, pure Rust.
//! - **Master keys**: 256-bit, from a key file or variable ([`Keyring`]),
//!   never from the TOML. Each has a [`KeyId`], a one-way fingerprint.
//! - **Per-segment key**: `HKDF-SHA-256(salt = segment salt, ikm = master,
//!   info = "norn segment key v1" ‖ stream ‖ first_seq ‖ generation)`. The
//!   24-byte salt is fresh from the OS random source for every segment file,
//!   so two files never share a key, even when a sequence number recurs (a
//!   quarantined header, a retried compaction).
//! - **Nonce**: the frame's byte offset in its file (`u64` LE, then four zero
//!   bytes). Offsets strictly increase as a writer appends, and a segment file
//!   is created exclusively, written once and never rewritten in place, so
//!   under one segment key no nonce is ever used twice.
//! - **Associated data**: the format version, stream, `first_seq`,
//!   generation, the offset, and the frame header's bytes 4..40 (kind, signal,
//!   time range, count, body length). Moving, relabelling or splicing a frame
//!   fails authentication.
//! - **What is encrypted**: chunk bodies, in every segment, which includes the
//!   write-ahead journal. Not encrypted: segment and frame headers and
//!   directories (stream and signal ids, time ranges, counts, sizes), index
//!   sidecars, clean markers, the metadata log and quarantined bytes beyond
//!   what they already were.

mod cipher;
mod error;
mod keyring;
mod rekey;
mod select;
mod store;

pub(crate) use cipher::SegmentCipher;
pub use cipher::TAG_LEN;
pub use error::{EncryptionError, KeyError};
pub use keyring::Keyring;
pub use rekey::{RekeyReport, rekey_database};
pub(crate) use select::cipher_for;
pub use store::{ENCRYPTION_FILE, check_database, check_segments, secure_database_dir};

use std::fmt;

/// Bytes in a master key.
pub const KEY_LEN: usize = 32;
/// Bytes in a segment's random salt.
pub const SALT_LEN: usize = 24;
/// The most keys one keyring holds: the current one and its predecessors.
pub const MAX_KEYS: usize = 16;

/// A master key's fingerprint: the first 8 bytes of
/// `HKDF-SHA-256(ikm = key, info = "norn key id v1")`. Stored in each
/// encrypted segment's header so a reader knows which key to use, and so a
/// wrong key is reported as such rather than as damage.
///
/// Concurrency: `Copy` plain data.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(pub [u8; 8]);

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({self})")
    }
}

/// What an encrypted segment's header records about its encryption: the key
/// and the salt its segment key was derived with.
///
/// Concurrency: `Copy` plain data. Neither field is secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncryptionInfo {
    /// The master key's id.
    pub key: KeyId,
    /// The segment's random HKDF salt.
    pub salt: [u8; SALT_LEN],
}

impl EncryptionInfo {
    /// Fresh parameters for a new segment under `key`.
    ///
    /// # Errors
    /// [`EncryptionError::Random`] if the OS random source fails.
    pub(crate) fn fresh(key: KeyId) -> Result<Self, EncryptionError> {
        let mut salt = [0u8; SALT_LEN];
        getrandom::fill(&mut salt).map_err(|_| EncryptionError::Random)?;
        Ok(Self { key, salt })
    }
}
