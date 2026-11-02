//! Reading and checking every chunk of a stream, without changing a byte.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use norn_core::StreamId;
use norn_storage::{RecoveryEvent, RecoveryMode, recover_segment, stream_dir};
use tokio_util::sync::CancellationToken;

use super::Maintenance;
use super::ops::{Failure, Operation};

/// The most damaged regions a report describes; the counts cover all.
const MAX_REPORTED: usize = 64;

/// One region that failed a check.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DamagedRegion {
    /// The segment's file name.
    pub segment: String,
    /// Where the region starts in the file.
    pub offset: u64,
    /// Its length in bytes.
    pub len: u64,
    /// What was wrong, for people.
    pub description: String,
}

/// What verifying a stream found.
///
/// Concurrency: owned plain data. Memory: at most 64 regions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct VerifyReport {
    /// Sealed segments read.
    pub segments: u64,
    /// Chunks read.
    pub chunks: u64,
    /// Samples their directories claim.
    pub samples: u64,
    /// Bytes read.
    pub bytes: u64,
    /// Regions that failed a check.
    pub damaged_chunks: u64,
    /// Samples those regions claim.
    pub damaged_samples: u64,
    /// The first of those regions.
    pub regions: Vec<DamagedRegion>,
}

impl Maintenance {
    /// Reads every chunk of `stream`'s sealed segments and checks both CRCs. The
    /// worker runs one job at a time, so no compaction or retention removes a file
    /// from under this. Damage is reported, and counted as `note_corruption` does.
    ///
    /// Cost: every byte, one frame in memory at a time. Reads go through the OS
    /// page cache, so this checks what the kernel returns.
    ///
    /// # Errors
    /// [`Failure::Cancelled`] between segments; [`Failure::Failed`] for an I/O
    /// error that stops a segment being assessed at all.
    pub(super) fn verify_stream(
        &self,
        stream: StreamId,
        op: &Operation,
        cancel: &CancellationToken,
    ) -> Result<VerifyReport, Failure> {
        let mut report = VerifyReport::default();
        let Some(index) = self.ingest.index().map(Arc::clone) else {
            return Ok(report);
        };
        let snapshot = index.snapshot();
        let records = snapshot.segments(stream);
        op.total.store(records.len() as u64, Ordering::Relaxed);
        let dir = stream_dir(self.ingest.root(), stream);
        let opts = self.ingest.storage_options();
        for record in records {
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            let path = dir.join(record.name().file_name());
            let found = recover_segment(&path, &opts, RecoveryMode::ReportOnly)
                .map_err(|e| Failure::Failed(e.to_string()))?;
            report.segments += 1;
            report.chunks += u64::from(record.chunks);
            report.samples += record.samples;
            report.bytes += record.file_len;
            for event in &found.events {
                let corruption = match event {
                    RecoveryEvent::Corruption { corruption, .. }
                    | RecoveryEvent::SegmentQuarantined { corruption, .. } => corruption,
                    _ => continue,
                };
                let before = self.ingest.stats(stream).corrupt_samples;
                self.ingest.note_corruption(stream, corruption);
                let after = self.ingest.stats(stream).corrupt_samples;
                report.damaged_chunks += 1;
                report.damaged_samples += after.saturating_sub(before);
                if report.regions.len() < MAX_REPORTED {
                    report.regions.push(DamagedRegion {
                        segment: record.name().file_name(),
                        offset: corruption.offset,
                        len: corruption.len,
                        description: format!("{:?}", corruption.kind),
                    });
                }
            }
            op.done.fetch_add(1, Ordering::Relaxed);
        }
        Ok(report)
    }
}
