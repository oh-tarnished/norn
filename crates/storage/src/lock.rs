//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One writer per stream, enforced by an OS file lock.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Result, StorageError};

#[cfg(test)]
mod tests;

/// The lock file's name inside a stream directory.
pub const LOCK_FILE: &str = "LOCK";

/// Exclusive ownership of a stream directory, for as long as this value lives.
///
/// An OS advisory lock on `<dir>/LOCK` (`File::try_lock`: `flock` on Unix,
/// `LockFileEx` on Windows). **Crash recovery needs nothing:** the operating
/// system releases the lock when the holder dies, however it dies, so no
/// stale-lock logic exists to get wrong; the file left behind means nothing.
/// Its contents are the holder's process ID, written for a human reading a
/// [`StorageError::Locked`] and never read back.
///
/// Advisory: it binds code that takes it (every [`crate::StreamWriter`] and
/// repairing recovery), not someone editing the directory by hand. A second
/// acquire in the same process fails too, since each acquire opens the file
/// anew. Network file systems may not honour it; a Norn database is local
/// (SPEC §46).
///
/// Memory: one file handle. Concurrency: `Send + Sync`; dropping it releases
/// the lock, and nothing else does.
#[derive(Debug)]
pub struct StreamLock {
    _file: File,
    path: PathBuf,
}

impl Drop for StreamLock {
    fn drop(&mut self) {
        // A child being spawned can briefly inherit the Unix open description.
        // Unlock it explicitly instead of waiting for its last handle to close.
        let _ = self._file.unlock();
    }
}

impl StreamLock {
    /// Takes the lock on directory `dir`, which must exist. Never waits.
    ///
    /// # Errors
    /// [`StorageError::Locked`] when another holder has it;
    /// [`StorageError::Io`] when the file cannot be opened or locked.
    pub fn acquire(dir: &Path) -> Result<Self> {
        let path = dir.join(LOCK_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| StorageError::io("open", &path, e))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(StorageError::Locked { path }),
            Err(TryLockError::Error(e)) => return Err(StorageError::io("lock", &path, e)),
        }
        let pid = format!("{}\n", std::process::id());
        let written = file
            .set_len(0)
            .and_then(|()| file.write_all(pid.as_bytes()));
        written.map_err(|e| StorageError::io("write", &path, e))?;
        Ok(Self { _file: file, path })
    }

    /// The lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
