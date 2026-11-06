//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Holding a whole database offline.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::{DatabaseError, Result};

/// The directory whose `LOCK` a daemon holds for as long as it runs.
pub const METADATA_DIR: &str = "metadata";

/// The database lock: an OS advisory lock on `<database>/metadata/LOCK`, the
/// same file the daemon's registry holds for its whole life
/// (docs/reference/storage.md, "Locking"). So an offline job and a daemon
/// exclude each other, whichever comes first, and a crashed holder leaves
/// nothing to clean up: the OS drops the lock with the process.
///
/// Concurrency: `Send + Sync`; dropping it releases the lock.
#[derive(Debug)]
pub struct DatabaseLock {
    file: File,
    path: PathBuf,
}

impl Drop for DatabaseLock {
    fn drop(&mut self) {
        // As norn-storage's StreamLock: an inherited descriptor must not keep it.
        let _ = self.file.unlock();
    }
}

impl DatabaseLock {
    /// Takes the lock of the database at `root`, creating `metadata/` if
    /// `create`. Never waits.
    ///
    /// # Errors
    /// [`DatabaseError::Locked`] while a daemon (or another job) holds it;
    /// [`DatabaseError::Io`], including for a missing database when not `create`.
    pub fn acquire(root: &Path, create: bool) -> Result<Self> {
        let dir = root.join(METADATA_DIR);
        if create {
            fs::create_dir_all(&dir).map_err(|e| DatabaseError::io("create", &dir, e))?;
        }
        let path = dir.join("LOCK");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| DatabaseError::io("open", &path, e))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file, path }),
            Err(TryLockError::WouldBlock) => Err(DatabaseError::Locked { path }),
            Err(TryLockError::Error(e)) => Err(DatabaseError::io("lock", path, e)),
        }
    }

    /// The lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
