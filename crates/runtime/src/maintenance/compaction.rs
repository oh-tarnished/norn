//! Compacting one stream: bounded runs, one at a time, on the worker's thread.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use norn_core::{StreamId, Timestamp};
use norn_index::SegmentRecord;
use norn_storage::{
    Bound, CompactionObserver, CompactionPhase, SegmentName, StorageError, compact_observed,
    stream_dir,
};
use tokio_util::sync::CancellationToken;

use super::Maintenance;
use super::ops::{Failure, Operation};
use super::plan::{RunLimits, next_run};

/// What a compaction did, over all its runs.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompactionTotals {
    /// Chunks in the inputs.
    pub original_chunks: u64,
    /// Chunks in the outputs.
    pub compacted_chunks: u64,
    /// File bytes of the inputs.
    pub original_bytes: u64,
    /// File bytes of the outputs.
    pub compacted_bytes: u64,
    /// Segments left alone because they hold known damage.
    pub skipped_segments: u64,
}

/// A run's link to its caller: cancellation, progress, and the test probe.
struct Watch<'a> {
    cancel: &'a CancellationToken,
    op: Option<&'a Operation>,
    probe: Option<&'a super::Probe>,
}

impl CompactionObserver for Watch<'_> {
    fn phase(&self, phase: CompactionPhase) {
        if let Some(probe) = self.probe {
            probe(phase);
        }
    }

    fn chunk_read(&self) {
        if let Some(op) = self.op {
            op.done.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl Maintenance {
    fn limits(&self) -> RunLimits {
        let opts = self.ingest.storage_options();
        RunLimits {
            max_bytes: opts.max_segment_bytes,
            max_span_nanos: opts.max_segment_span_nanos,
        }
    }

    fn skipped(&self, stream: StreamId, name: SegmentName) -> bool {
        let refused = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
        refused.contains(&(stream, name)) || self.ingest.is_damaged(stream, name)
    }

    /// Compacts `stream` until nothing is left to do or `max_runs` have run.
    ///
    /// Damage found in an input is recorded and the segment left alone, so it does
    /// not fail the rest; so is a run too large for one segment.
    ///
    /// # Errors
    /// [`Failure::Cancelled`]; [`Failure::Failed`] for anything else. Either way
    /// each finished run stands, and the unfinished one left its inputs live.
    pub(super) fn compact_stream(
        &self,
        stream: StreamId,
        interval: Option<(Timestamp, Timestamp)>,
        op: Option<&Operation>,
        cancel: &CancellationToken,
        max_runs: usize,
    ) -> Result<CompactionTotals, Failure> {
        let mut totals = CompactionTotals::default();
        let Some(index) = self.ingest.index().map(Arc::clone) else {
            return Ok(totals);
        };
        let watch = Watch {
            cancel,
            op,
            probe: self.probe.get(),
        };
        let mut runs = 0;
        while runs < max_runs {
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            let snapshot = index.snapshot();
            let records = snapshot.segments(stream);
            let skip = |name| self.skipped(stream, name);
            let run = next_run(records, self.limits(), interval, &skip);
            if run.is_empty() {
                let held = records.iter().filter(|r| skip(r.name())).count();
                totals.skipped_segments = held as u64;
                break;
            }
            if let Some(op) = op {
                let chunks: u64 = run.iter().map(|r| u64::from(r.chunks)).sum();
                op.total.fetch_add(chunks, Ordering::Relaxed);
            }
            self.compact_run(stream, &run, &watch, &mut totals)?;
            runs += 1;
        }
        Ok(totals)
    }

    fn compact_run(
        &self,
        stream: StreamId,
        run: &[SegmentRecord],
        watch: &Watch<'_>,
        totals: &mut CompactionTotals,
    ) -> Result<(), Failure> {
        let dir = stream_dir(self.ingest.root(), stream);
        let inputs: Vec<_> = run.iter().map(|r| dir.join(r.name().file_name())).collect();
        let opts = self.ingest.storage_options();
        let _memory = self.reserve(run)?;
        match compact_observed(&dir, &inputs, &opts, watch) {
            Ok(outcome) => {
                totals.original_chunks += outcome.chunks_before as u64;
                totals.original_bytes += outcome.bytes_before;
                totals.compacted_chunks += outcome.output.chunks as u64;
                totals.compacted_bytes += outcome.output.bytes;
                self.counters.compacted(run.len() as u64, &outcome);
                tracing::info!(
                    stream = stream.get(),
                    inputs = run.len(),
                    chunks_before = outcome.chunks_before,
                    chunks_after = outcome.output.chunks,
                    bytes_before = outcome.bytes_before,
                    bytes_after = outcome.output.bytes,
                    "compacted"
                );
                if let Err(e) = self.ingest.compacted(stream, &outcome) {
                    tracing::warn!(error = %e, "index behind a compaction; refreshing");
                    self.refresh(stream);
                }
                Ok(())
            }
            Err(StorageError::CompactionCancelled) => Err(Failure::Cancelled),
            Err(StorageError::Corrupt(found)) => {
                self.ingest.note_corruption(stream, &found);
                Ok(())
            }
            Err(StorageError::Codec { path, offset, .. }) => {
                if let Some(name) = SegmentName::parse(&path) {
                    self.ingest.note_damage(stream, name, offset, 0);
                }
                Ok(())
            }
            Err(StorageError::TooLarge {
                bound: Bound::SegmentChunks,
                ..
            }) => {
                let mut refused = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
                refused.extend(run.iter().map(|r| (stream, r.name())));
                Ok(())
            }
            Err(e) => {
                self.counters.failed();
                tracing::error!(stream = stream.get(), error = %e, "compaction failed");
                self.refresh(stream);
                Err(Failure::Failed(e.to_string()))
            }
        }
    }
}
