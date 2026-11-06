//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The master keys: parsed from a key file or variable, never from the TOML.

use std::fmt;
use std::io::Read;
use std::path::Path;

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::error::KeyError;
use super::{KEY_LEN, KeyId, MAX_KEYS};

/// The largest key file read: [`MAX_KEYS`] lines with room for comments.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// One 256-bit master key and its id. Wiped from memory when dropped.
pub(crate) struct MasterKey {
    pub(crate) id: KeyId,
    pub(crate) bytes: Zeroizing<[u8; KEY_LEN]>,
}

/// The master keys a daemon holds. The first is **current**: every new segment
/// is encrypted with it. The others are older keys, kept only so segments
/// written under them stay readable until `nornctl rekey` has rewritten them.
///
/// Format, for a key file and for a variable alike: keys of 64 hexadecimal
/// digits, separated by newlines, spaces or commas; `#` starts a comment. A key
/// is identified by [`KeyId`], a one-way fingerprint, never by its bytes.
///
/// Memory: 40 bytes a key, at most [`MAX_KEYS`]. Keys are zeroed on drop; a
/// keyring [`leak`](Keyring::leak)ed for the life of the process is never
/// dropped, and goes with the process. Concurrency: `Send + Sync`, read-only.
pub struct Keyring {
    keys: Vec<MasterKey>,
}

impl Keyring {
    /// Parses `text`. Copies the keys; borrows nothing past the call.
    ///
    /// # Errors
    /// [`KeyError::Empty`], [`KeyError::Malformed`], [`KeyError::Duplicate`]
    /// or [`KeyError::TooMany`]. No error contains key material.
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let mut keys: Vec<MasterKey> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line_no = n + 1;
            let body = line.split('#').next().unwrap_or_default();
            for token in body.split([' ', '\t', ',', '\r']).filter(|t| !t.is_empty()) {
                let bytes = decode_hex(token).ok_or(KeyError::Malformed { line: line_no })?;
                let id = KeyId::of(&bytes);
                if keys.iter().any(|k| k.id == id) {
                    return Err(KeyError::Duplicate { line: line_no });
                }
                if keys.len() >= MAX_KEYS {
                    return Err(KeyError::TooMany);
                }
                keys.push(MasterKey { id, bytes });
            }
        }
        if keys.is_empty() {
            return Err(KeyError::Empty);
        }
        Ok(Self { keys })
    }

    /// Reads and parses the key file at `path`, which must be a regular file
    /// readable by its owner only (Unix mode `0600` or `0400`). On Windows no
    /// mode exists to check; the file's ACL is the operator's to restrict.
    ///
    /// # Errors
    /// [`KeyError::Insecure`] for a group- or world-readable file,
    /// [`KeyError::Unreadable`], or any error of [`Keyring::parse`].
    pub fn from_file(path: &Path) -> Result<Self, KeyError> {
        let unreadable = |e: std::io::Error| KeyError::Unreadable {
            path: path.to_path_buf(),
            reason: e.to_string(),
        };
        let file = std::fs::File::open(path).map_err(unreadable)?;
        let meta = file.metadata().map_err(unreadable)?;
        if let Some(mode) = crate::platform::exposed_mode(&meta) {
            let path = path.to_path_buf();
            return Err(KeyError::Insecure { path, mode });
        }
        let mut text = Zeroizing::new(String::new());
        let mut limited = file.take(MAX_FILE_BYTES);
        limited.read_to_string(&mut text).map_err(unreadable)?;
        Self::parse(&text)
    }

    /// Reads and parses the variable `name`. The variable itself stays in this
    /// process's environment: std cannot clear it safely once threads run.
    ///
    /// # Errors
    /// [`KeyError::Empty`] when it is unset or not UTF-8, or any error of
    /// [`Keyring::parse`].
    pub fn from_env(name: &str) -> Result<Self, KeyError> {
        let text = std::env::var(name).map_err(|_| KeyError::Empty)?;
        Self::parse(&Zeroizing::new(text))
    }

    /// Keeps the keyring for the life of the process, so a `Copy`
    /// [`crate::StorageOptions`] can refer to it. Call it once, at startup.
    #[must_use]
    pub fn leak(self) -> &'static Self {
        Box::leak(Box::new(self))
    }

    /// The id of the current key, the one new segments are encrypted with.
    #[must_use]
    pub fn current(&self) -> KeyId {
        self.keys.first().map_or(KeyId([0; 8]), |k| k.id)
    }

    /// Every key's id, current first.
    pub fn ids(&self) -> impl Iterator<Item = KeyId> + '_ {
        self.keys.iter().map(|k| k.id)
    }

    /// The key with `id`.
    pub(crate) fn get(&self, id: KeyId) -> Option<&MasterKey> {
        self.keys.iter().find(|k| k.id == id)
    }

    /// The current key.
    pub(crate) fn current_key(&self) -> Option<&MasterKey> {
        self.keys.first()
    }
}

impl KeyId {
    /// The fingerprint of `key`: the first 8 bytes of HKDF-SHA-256 with info
    /// `norn key id v1`. One-way: it says which key, and nothing of the key.
    pub(crate) fn of(key: &[u8; KEY_LEN]) -> Self {
        let mut out = [0u8; 8];
        let hk = Hkdf::<Sha256>::new(None, key);
        // 8 bytes is far below HKDF's 255 × 32 limit, so expand cannot fail.
        let _ = hk.expand(b"norn key id v1", &mut out);
        Self(out)
    }
}

impl PartialEq for Keyring {
    fn eq(&self, other: &Self) -> bool {
        self.ids().eq(other.ids())
    }
}

impl Eq for Keyring {}

impl fmt::Debug for Keyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<String> = self.ids().map(|id| id.to_string()).collect();
        f.debug_struct("Keyring").field("ids", &ids).finish()
    }
}

fn decode_hex(token: &str) -> Option<Zeroizing<[u8; KEY_LEN]>> {
    let digits = token.as_bytes();
    if digits.len() != KEY_LEN * 2 {
        return None;
    }
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    for (slot, [hi, lo]) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
        let hi = char::from(*hi).to_digit(16)?;
        let lo = char::from(*lo).to_digit(16)?;
        *slot = u8::try_from(hi * 16 + lo).ok()?;
    }
    Some(out)
}
