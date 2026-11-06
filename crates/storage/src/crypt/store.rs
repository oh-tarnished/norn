//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A database as a whole: is it encrypted, can this configuration open it, and
//! who may enter its directory.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{EncryptionError, Keyring};
use crate::error::{Result, StorageError};
use crate::fsio::atomic_write;
use crate::layout::list_segments_lenient;

/// The marker at `<database>/ENCRYPTION`: the database's segments are encrypted.
/// It holds no key material, and it keeps an encrypted database with no segments
/// yet from being taken for a plain one.
pub const ENCRYPTION_FILE: &str = "ENCRYPTION";

const MARKER: &[u8] = b"norn encrypted database v1\n";

/// Checks that `keys`, or their absence, can open the database at `root`, and
/// records a database that is now encrypted. Reads one marker and one header per
/// segment. Touches no segment, so a refused start changes nothing.
///
/// # Errors
/// [`EncryptionError::Mismatch`] for an encrypted database with no keys, or a
/// plain one with keys; [`EncryptionError::KeyUnavailable`] for a segment under a
/// key `keys` does not hold; [`StorageError::Io`].
pub fn check_database(root: &Path, keys: Option<&Keyring>) -> Result<()> {
    let marker = root.join(ENCRYPTION_FILE);
    let marked = match fs::read(&marker) {
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(StorageError::io("read", marker, e)),
    };
    if marked && keys.is_none() {
        return Err(mismatch(root.to_path_buf(), true));
    }
    check_segments(root, keys)?;
    if !marked && keys.is_some() {
        fs::create_dir_all(root).map_err(|e| StorageError::io("create directory", root, e))?;
        atomic_write(&marker, MARKER)?;
    }
    Ok(())
}

/// Checks every segment header under `root`, journals included, against `keys`.
/// A header too damaged to read is left for recovery, which reports it.
///
/// # Errors
/// As [`check_database`].
pub fn check_segments(root: &Path, keys: Option<&Keyring>) -> Result<()> {
    for dir in segment_dirs(root)? {
        for entry in list_segments_lenient(&dir)? {
            match (&entry.header.encryption, keys) {
                (None, None) => {}
                (None, Some(_)) => return Err(mismatch(entry.path, false)),
                (Some(_), None) => return Err(mismatch(entry.path, true)),
                (Some(info), Some(keys)) if keys.get(info.key).is_none() => {
                    let (path, key) = (entry.path, info.key);
                    let missing = EncryptionError::KeyUnavailable { path, key };
                    return Err(StorageError::Encryption(missing));
                }
                (Some(_), Some(_)) => {}
            }
        }
    }
    Ok(())
}

fn mismatch(path: PathBuf, encrypted: bool) -> StorageError {
    StorageError::Encryption(EncryptionError::Mismatch { path, encrypted })
}

/// Every directory under `root` that can hold segments: each stream's, and its
/// journal's.
pub(crate) fn segment_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let streams = root.join("streams");
    let entries = match fs::read_dir(&streams) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(StorageError::io("list", streams, e)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| StorageError::io("list", &streams, e))?
            .path();
        if path.is_dir() {
            let journal = path.join("journal");
            if journal.is_dir() {
                dirs.push(journal);
            }
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Makes the database directory `root` owner-only (Unix mode `0700`): creates it
/// so, with any missing parents, or tightens one that exists. Files inside are
/// created under the umask, usually readable by anyone, so this one directory is
/// what keeps other users out of the data. Returns the mode it had when it had to
/// be tightened; the caller says so in its log. No-op on Windows, where the
/// profile directory's inherited ACL applies.
///
/// # Errors
/// [`StorageError::Io`] when it cannot be created or restricted, as when another
/// user owns it.
pub fn secure_database_dir(root: &Path) -> Result<Option<u32>> {
    crate::platform::owner_only_dir(root)
        .map_err(|e| StorageError::io("restrict directory", root, e))
}
