//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A copy of the catalogue at one commit point, for an online backup.

use std::path::Path;
use std::sync::PoisonError;

use super::Registry;
use crate::error::MetadataError;
use crate::log::write_log;

impl Registry {
    /// Writes every live record, as of one instant, to a new log at `path` and
    /// fsyncs it: the log the daemon would have after a rewrite, which a
    /// registry opened over it replays to exactly this state.
    ///
    /// The instant is a commit point: the shared lock on the records is held
    /// while they are listed, so a batch is in whole or not at all, and no
    /// mutation is half applied. A mutation visible but not yet fsynced in the
    /// live log is included; it was visible to every reader, and whatever names
    /// it was admitted after it. The live log is not touched, so this neither
    /// waits for nor delays an fsync.
    ///
    /// Memory: the records, encoded once more. Blocks on the disk: call it from a
    /// blocking context. The file's name is not made durable; rename it into
    /// place and sync its directory.
    ///
    /// # Errors
    /// [`MetadataError::Io`].
    pub fn snapshot_to(&self, path: &Path) -> Result<(), MetadataError> {
        let ops = self
            .state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot();
        drop(write_log(path, &ops)?);
        Ok(())
    }
}
