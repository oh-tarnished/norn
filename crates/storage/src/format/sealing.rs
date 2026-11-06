//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The encryption record: 48 bytes right after the header of a format 1.1
//! segment whose header flag 0x01 is set. Its absence is format 1.0.
//!
//! | Offset | Size | Field |
//! | ---: | ---: | --- |
//! | 48 | 4 | magic `NENC` |
//! | 52 | 1 | cipher: 1, ChaCha20-Poly1305 |
//! | 53 | 1 | key derivation: 1, HKDF-SHA-256 |
//! | 54 | 2 | reserved, zero |
//! | 56 | 8 | key id |
//! | 64 | 24 | salt |
//! | 88 | 4 | reserved, zero |
//! | 92 | 4 | CRC-32C of bytes 48..92 |
//!
//! Nothing here is secret: the key id is a one-way fingerprint and the salt is
//! public by design. Tampering with either derives a different segment key,
//! so every chunk then fails authentication.

use std::path::Path;

use super::{HEADER_LEN, Le};
use crate::crc32c::crc32c;
use crate::crypt::{EncryptionError, EncryptionInfo, KeyId};
use crate::error::{CorruptionKind, Result, StorageError};

/// Bytes in the encryption record.
pub const SEALING_LEN: usize = 48;
/// First four bytes of the encryption record.
pub const SEALING_MAGIC: [u8; 4] = *b"NENC";
/// The cipher byte for ChaCha20-Poly1305.
const CHACHA20_POLY1305: u8 = 1;
/// The key derivation byte for HKDF-SHA-256.
const HKDF_SHA256: u8 = 1;

/// The record's bytes for `info`, CRC included.
pub(crate) fn encode(info: &EncryptionInfo) -> [u8; SEALING_LEN] {
    let mut b = Vec::with_capacity(SEALING_LEN);
    b.extend_from_slice(&SEALING_MAGIC);
    b.extend([CHACHA20_POLY1305, HKDF_SHA256, 0, 0]);
    b.extend_from_slice(&info.key.0);
    b.extend_from_slice(&info.salt);
    b.extend_from_slice(&0u32.to_le_bytes());
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    let mut out = [0u8; SEALING_LEN];
    out.copy_from_slice(&b);
    out
}

/// Parses the record from `bytes`, the [`SEALING_LEN`] bytes after the
/// header of the segment at `path`. Copies; borrows nothing past the call.
///
/// # Errors
/// [`StorageError::Corrupt`] (`TornTail` when short, `HeaderChecksum` for a
/// bad magic or CRC), reported over the whole 96-byte header so recovery
/// quarantines the file as it would a torn header; and
/// [`EncryptionError::UnsupportedCipher`] for an intact record naming a
/// cipher this build does not have.
pub(crate) fn decode(bytes: &[u8], path: &Path) -> Result<EncryptionInfo> {
    let len = (HEADER_LEN + SEALING_LEN) as u64;
    let bad = |kind| StorageError::corrupt(path, 0, len, kind);
    let rec = bytes
        .get(..SEALING_LEN)
        .ok_or(bad(CorruptionKind::TornTail))?;
    let (body, crc) = rec.split_at(SEALING_LEN - 4);
    let mut r = Le::new(rec);
    if r.bytes::<4>() != Some(SEALING_MAGIC) || crc != crc32c(body).to_le_bytes() {
        return Err(bad(CorruptionKind::HeaderChecksum));
    }
    let malformed = || bad(CorruptionKind::Malformed("encryption record"));
    let cipher = r.u8().ok_or_else(malformed)?;
    let kdf = r.u8().ok_or_else(malformed)?;
    let _reserved = r.u16();
    if (cipher, kdf) != (CHACHA20_POLY1305, HKDF_SHA256) {
        let path = path.to_path_buf();
        let found = (cipher, kdf);
        return Err(StorageError::Encryption(
            EncryptionError::UnsupportedCipher { path, found },
        ));
    }
    let key = KeyId(r.bytes::<8>().ok_or_else(malformed)?);
    let salt = r.bytes::<24>().ok_or_else(malformed)?;
    Ok(EncryptionInfo { key, salt })
}
