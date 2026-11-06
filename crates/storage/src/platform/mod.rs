//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The one platform difference storage has: making a directory entry
//! durable (SPEC §5: platform code behind one function, not scattered `cfg`).
//!
//! On Unix a new, renamed or removed file's *name* is durable only once its
//! directory is fsynced; that is what [`sync_dir`] does. Windows has no
//! portable directory fsync: NTFS journals metadata, and `std` cannot open a
//! directory handle without `FILE_FLAG_BACKUP_SEMANTICS`, so there it is a
//! documented no-op (docs/reference/storage.md "Durability").

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

use std::path::{Path, PathBuf};

use crate::error::{Result, StorageError};

/// Makes the entries of directory `dir` (creations, renames, removals)
/// durable. Blocks for one fsync on Unix; returns at once on Windows.
///
/// # Errors
/// [`StorageError::Io`] when the directory cannot be opened or synced.
pub fn sync_dir(dir: &Path) -> Result<()> {
    imp::sync_dir(dir).map_err(|e| StorageError::io("fsync directory", dir, e))
}

/// Creates `dir` and any missing parents, then makes each new name durable by
/// syncing the directory holding it: without that, a power cut can lose a whole
/// new stream directory, and with it segments that were already fsynced. Returns
/// the directories synced. A directory left behind by a process that died before
/// reaching here is not resynced; its next power cut is covered by the OS's own
/// writeback only.
///
/// # Errors
/// [`StorageError::Io`] when creating or syncing fails.
pub fn create_dir_durable(dir: &Path) -> Result<Vec<PathBuf>> {
    let missing: Vec<&Path> = dir.ancestors().take_while(|d| !d.exists()).collect();
    std::fs::create_dir_all(dir).map_err(|e| StorageError::io("create directory", dir, e))?;
    let synced: Vec<PathBuf> = missing
        .iter()
        .map(|d| crate::fsio::parent(d).to_path_buf())
        .collect();
    synced.iter().try_for_each(|d| sync_dir(d))?;
    Ok(synced)
}

/// The permission bits of a file a key may not be read from: anything other than
/// a regular file its owner alone can reach. `None` when it is fine, and always
/// on Windows, which has no mode bits to read.
pub(crate) fn exposed_mode(meta: &std::fs::Metadata) -> Option<u32> {
    imp::exposed_mode(meta)
}

/// Makes `dir` owner-only, creating it if missing; the mode it had if that took
/// a change. See [`crate::crypt::secure_database_dir`].
pub(crate) fn owner_only_dir(dir: &Path) -> std::io::Result<Option<u32>> {
    imp::owner_only_dir(dir)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn every_new_directory_has_its_parent_synced_and_old_ones_are_left_alone() {
        let id = std::process::id();
        let base = std::env::temp_dir().join(format!("norn-durable-{id}"));
        let leaf = base.join("a/b");
        let synced = create_dir_durable(&leaf).unwrap();
        assert!(leaf.is_dir());
        assert_eq!(synced, [base.join("a"), base.clone(), std::env::temp_dir()]);
        assert!(create_dir_durable(&leaf).unwrap().is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
