//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An online backup, as one maintenance job.
//!
//! Being a job is what makes it safe: compaction and retention run on this worker too,
//! so none removes a segment this is copying, and they wait until it finishes.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use norn_core::Timestamp;
use norn_database::{BackupWriter, COPY_BUFFER_BYTES};
use tokio_util::sync::CancellationToken;

use super::Maintenance;
use super::ops::{Failure, Operation};

/// What to back up to.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupSpec {
    /// The directory to write: absolute, and absent or empty.
    pub to: PathBuf,
    /// A finished backup to build on.
    pub base: Option<PathBuf>,
    /// The ID recorded in the manifest.
    pub id: String,
}

impl BackupSpec {
    /// A backup into `to` called `id`, incremental on `base` if given.
    #[must_use]
    pub fn new(to: PathBuf, base: Option<PathBuf>, id: String) -> Self {
        Self { to, base, id }
    }
}

/// What a finished backup holds.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupReport {
    /// When the cuts were taken, on the daemon's clock.
    pub cut: Timestamp,
    /// Streams backed up.
    pub streams: u64,
    /// Files the manifest lists.
    pub files: u64,
    /// Bytes those files take.
    pub bytes: u64,
    /// Files this backup copied.
    pub copied_files: u64,
    /// Bytes this backup copied.
    pub copied_bytes: u64,
}

fn failed(e: impl ToString) -> Failure {
    Failure::Failed(e.to_string())
}

impl Maintenance {
    /// Takes a backup: cuts every stream, writes the catalogue as of just after those
    /// cuts, so it names everything they hold, then copies the sealed segments and
    /// writes the manifest. Stops between files when cancelled, leaving no manifest.
    ///
    /// Cost: every new sealed byte, one buffer in memory. Reads no chunk.
    ///
    /// # Errors
    /// [`Failure::Cancelled`]; [`Failure::Failed`] with the reason, for a destination
    /// in use, an unreadable base, a failed stream or a full disk.
    pub(super) fn backup(
        &self,
        spec: &BackupSpec,
        op: &Operation,
        cancel: &CancellationToken,
    ) -> Result<BackupReport, Failure> {
        let _buffer = self
            .memory
            .try_reserve(COPY_BUFFER_BYTES as u64)
            .map_err(failed)?;
        let cut = self.clock.now();
        let base = spec.base.as_deref();
        let mut writer =
            BackupWriter::begin(&spec.to, base, cut.as_nanos(), &spec.id).map_err(failed)?;
        let ids = self.registry.view().stream_ids();
        let mut pending = Vec::with_capacity(ids.len());
        for id in &ids {
            pending.push((*id, self.ingest.begin_cut(*id, cancel).map_err(failed)?));
        }
        let mut streams = Vec::with_capacity(pending.len());
        for (id, cut) in pending {
            streams.push((id, cut.wait(cancel).map_err(failed)?));
        }
        let log = writer.log_path().map_err(failed)?;
        self.registry.snapshot_to(&log).map_err(failed)?;
        writer.add_log().map_err(failed)?;
        let total: usize = streams.iter().map(|(_, s)| s.len()).sum();
        op.total.store(total as u64 + 1, Ordering::Relaxed);
        op.done.store(1, Ordering::Relaxed);
        for (id, segments) in &streams {
            for segment in segments {
                if cancel.is_cancelled() {
                    return Err(Failure::Cancelled);
                }
                writer.add_segment(*id, segment).map_err(failed)?;
                op.done.fetch_add(1, Ordering::Relaxed);
            }
        }
        let summary = writer.finish().map_err(failed)?;
        tracing::info!(backup = %spec.id, files = summary.files, "backup finished");
        Ok(BackupReport {
            cut,
            streams: streams.len() as u64,
            files: summary.files,
            bytes: summary.bytes,
            copied_files: summary.copied_files,
            copied_bytes: summary.copied_bytes,
        })
    }
}
