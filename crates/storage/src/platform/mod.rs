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

use std::path::Path;

use crate::error::{Result, StorageError};

/// Makes the entries of directory `dir` (creations, renames, removals)
/// durable. Blocks for one fsync on Unix; returns at once on Windows.
///
/// # Errors
/// [`StorageError::Io`] when the directory cannot be opened or synced.
pub fn sync_dir(dir: &Path) -> Result<()> {
    imp::sync_dir(dir).map_err(|e| StorageError::io("fsync directory", dir, e))
}
