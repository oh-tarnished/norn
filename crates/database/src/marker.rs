//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `<database>/FORMAT`: the store version, in 16 bytes.
//!
//! | Offset | Size | Field |
//! | ---: | ---: | --- |
//! | 0 | 8 | magic `NORNDB\0\0` |
//! | 8 | 2 | store version |
//! | 10 | 2 | reserved, zero |
//! | 12 | 4 | CRC-32C of bytes 0..12 |
//!
//! Written atomically (temp, fsync, rename, fsync directory). A database
//! without one was written by 1.0.0-rc.1 or earlier, which is store version 1
//! ([`LEGACY_VERSION`](crate::LEGACY_VERSION)).

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use norn_storage::{atomic_write, crc32c};

use crate::error::{DatabaseError, Result};

/// The marker's file name inside a database directory.
pub const FORMAT_FILE: &str = "FORMAT";
/// Present while `nornctl restore` writes a database; the daemon refuses it.
pub const RESTORING_FILE: &str = "RESTORING";

const MAGIC: &[u8; 8] = b"NORNDB\0\0";
const LEN: usize = 16;

/// The marker's bytes for `version`, CRC included.
#[must_use]
pub fn encode(version: u16) -> [u8; LEN] {
    let mut out = [0u8; LEN];
    let mut b = Vec::with_capacity(LEN);
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&version.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    out.copy_from_slice(&b);
    out
}

/// The store version in `root`'s marker, or `None` when it has none. Copies.
///
/// # Errors
/// [`DatabaseError::Corrupt`] for a marker that is short, of another magic, or
/// fails its CRC: a bit flip is never read as a version; [`DatabaseError::Io`].
pub fn read(root: &Path) -> Result<Option<u16>> {
    let path = root.join(FORMAT_FILE);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(DatabaseError::io("read", path, e)),
    };
    let bad = |what: &str| DatabaseError::corrupt(&path, what);
    let (body, crc) = bytes
        .get(..LEN)
        .filter(|_| bytes.len() == LEN)
        .ok_or_else(|| bad("FORMAT is not 16 bytes"))?
        .split_at(LEN - 4);
    if body.get(..8) != Some(MAGIC.as_slice()) {
        return Err(bad("FORMAT has the wrong magic"));
    }
    if crc != crc32c(body).to_le_bytes() {
        return Err(bad("FORMAT fails its CRC-32C"));
    }
    let version = body
        .get(8..10)
        .and_then(|v| <[u8; 2]>::try_from(v).ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| bad("FORMAT is short"))?;
    Ok(Some(version))
}

/// Records `version` in `root`'s marker, atomically.
///
/// # Errors
/// [`DatabaseError::Storage`] from the atomic write.
pub fn write(root: &Path, version: u16) -> Result<()> {
    Ok(atomic_write(&root.join(FORMAT_FILE), &encode(version))?)
}
