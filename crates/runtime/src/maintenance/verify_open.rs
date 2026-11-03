//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Verifying what a stream has written to its open segment.
//!
//! An open segment has no directory until it seals, so it is read the way a
//! query reads it: chunk by chunk, at the offsets the index was told after each
//! fsync. Each frame's header is checked against that metadata and its body
//! against its CRC. Without this, a stream that never filled or aged a segment
//! verified as zero chunks: a check that examined nothing, reported as a pass.

use std::path::Path;
use std::sync::atomic::Ordering;

use norn_core::StreamId;
use norn_index::ChunkLocator;
use norn_storage::{ChunkFile, SegmentName, StorageError};
use tokio_util::sync::CancellationToken;

use super::Maintenance;
use super::ops::{Failure, Operation};
use super::verify::{DamagedRegion, MAX_REPORTED, VerifyReport};

/// How many distinct segments `open` names; they come segment by segment.
pub(super) fn open_segment_count(open: &[ChunkLocator]) -> usize {
    let mut last: Option<SegmentName> = None;
    let mut n = 0;
    for l in open {
        if last != Some(l.segment) {
            n += 1;
            last = Some(l.segment);
        }
    }
    n
}

impl Maintenance {
    /// Reads and checks every chunk of `open`, the snapshot's tail chunks of
    /// `stream`, from the files in `dir`. Every chunk listed was fsynced before
    /// the index heard of it, so the file opened after the snapshot holds it:
    /// a short read is damage, not a race. Sealing between the snapshot and the
    /// read changes no chunk's bytes or offset.
    ///
    /// # Errors
    /// [`Failure::Cancelled`] between chunks; [`Failure::Failed`] when a segment
    /// cannot be opened at all.
    pub(super) fn verify_open(
        &self,
        stream: StreamId,
        dir: &Path,
        open: &[ChunkLocator],
        report: &mut VerifyReport,
        op: &Operation,
        cancel: &CancellationToken,
    ) -> Result<(), Failure> {
        let opts = self.ingest.storage_options();
        let mut file: Option<(SegmentName, ChunkFile)> = None;
        for l in open {
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            let fresh = file.as_ref().is_none_or(|(name, _)| *name != l.segment);
            if fresh {
                if file.is_some() {
                    op.done.fetch_add(1, Ordering::Relaxed);
                }
                let path = dir.join(l.segment.file_name());
                let opened = ChunkFile::open(&path, opts);
                let opened = opened.map_err(|e| Failure::Failed(e.to_string()))?;
                file = Some((l.segment, opened));
                report.open_segments += 1;
            }
            let Some((_, reader)) = file.as_mut() else {
                continue;
            };
            let samples = u64::from(l.chunk.count);
            report.chunks += 1;
            report.open_chunks += 1;
            report.samples += samples;
            report.open_samples += samples;
            report.bytes += l.chunk.frame_len();
            let region = match reader.read_chunk(&l.chunk) {
                Ok(_) => continue,
                Err(StorageError::Corrupt(c)) => (c.offset, c.len, format!("{:?}", c.kind)),
                Err(e @ StorageError::Codec { .. }) => {
                    (l.offset(), l.chunk.frame_len(), e.to_string())
                }
                Err(e) => return Err(Failure::Failed(e.to_string())),
            };
            self.damaged_open(stream, l, region, report);
        }
        if file.is_some() {
            op.done.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Counts the open-segment chunk at `l` as damaged, once, and describes
    /// `region` (offset, length, what was wrong) if the report has room.
    fn damaged_open(
        &self,
        stream: StreamId,
        l: &ChunkLocator,
        region: (u64, u64, String),
        report: &mut VerifyReport,
    ) {
        let before = self.ingest.stats(stream).corrupt_samples;
        let samples = u64::from(l.chunk.count);
        self.ingest
            .note_damage(stream, l.segment, l.offset(), samples);
        let after = self.ingest.stats(stream).corrupt_samples;
        report.damaged_chunks += 1;
        report.damaged_samples += after.saturating_sub(before);
        if report.regions.len() < MAX_REPORTED {
            let (offset, len, description) = region;
            report.regions.push(DamagedRegion {
                segment: l.segment.file_name(),
                offset,
                len,
                description,
            });
        }
    }
}
