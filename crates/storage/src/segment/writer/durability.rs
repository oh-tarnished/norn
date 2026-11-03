//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How a segment's bytes become durable: flush, sync, and the final seal.

use std::io::Write;

use super::SegmentWriter;
use crate::error::{Result, StorageError};
use crate::format::directory;
use crate::fsio::parent;
use crate::platform::sync_dir;
use crate::segment::SegmentSummary;

impl SegmentWriter {
    /// Writes buffered bytes to the OS. They survive a process crash but
    /// not a power loss.
    ///
    /// # Errors
    /// [`StorageError::Io`]; the writer is then poisoned.
    pub fn flush(&mut self) -> Result<()> {
        let r = self.out.flush();
        r.map_err(|e| self.poison("write", e))
    }

    /// Flushes, then fsyncs the file's data, and, the first time, its
    /// directory, so the segment's name is durable too. Everything appended
    /// before the call survives power loss once it returns.
    ///
    /// # Errors
    /// [`StorageError::Io`]; the writer is then poisoned, since a failed
    /// fsync leaves the kernel's page state unknown.
    pub fn sync(&mut self) -> Result<()> {
        self.flush()?;
        let r = self.out.get_ref().sync_data();
        r.map_err(|e| self.poison("fsync", e))?;
        if !self.dir_synced {
            sync_dir(parent(&self.path))?;
            self.dir_synced = true;
        }
        Ok(())
    }

    /// Writes the directory and trailer, fsyncs, and closes the segment.
    ///
    /// # Errors
    /// [`StorageError::Sealed`] when poisoned; [`StorageError::Io`]. The
    /// segment then reads as unsealed, and recovery seals it.
    pub fn seal(mut self) -> Result<SegmentSummary> {
        if self.poisoned {
            return Err(StorageError::Sealed);
        }
        // Barrier: the chunks reach the disk before the seal that vouches for
        // them, so a power loss can never leave a valid directory over chunks
        // that were not persisted.
        self.sync()?;
        self.frame.clear();
        directory::encode(&self.chunks, self.len, &mut self.frame);
        let r = self.out.write_all(&self.frame);
        r.map_err(|e| self.poison("write", e))?;
        self.len += self.frame.len() as u64;
        self.sync()?;
        Ok(SegmentSummary::new(
            self.path,
            self.header,
            &self.chunks,
            self.len,
        ))
    }

    fn poison(&mut self, op: &'static str, e: std::io::Error) -> StorageError {
        self.poisoned = true;
        StorageError::io(op, &self.path, e)
    }
}
