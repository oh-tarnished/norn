//! Telling the temporal index what was stored, and what its refusals mean.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use norn_core::StreamId;
use norn_index::{IndexError, TemporalIndex};
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{
    Appended, CompactionOutcome, SegmentName, SegmentSummary, StorageError, StreamWriter,
};

use super::Ingest;
use super::shared::Shared;

/// Why startup recovery failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum RecoverError {
    /// A stream could not be assessed.
    Storage(StorageError),
    /// The index could not open.
    Index(IndexError),
}

impl From<StorageError> for RecoverError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

impl From<IndexError> for RecoverError {
    fn from(e: IndexError) -> Self {
        Self::Index(e)
    }
}

impl fmt::Display for RecoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "{e}"),
            Self::Index(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RecoverError {}

/// Whether an index update took effect, logging what does not need more.
fn took_effect(stream: StreamId, result: Result<(), IndexError>) -> Result<(), IndexError> {
    match result {
        Err(IndexError::PersistFailed(e)) => {
            tracing::warn!(stream = stream.get(), error = %e, "index summary not written");
            Ok(())
        }
        other => other,
    }
}

/// The store thread's link to the index. Holds the metas of chunks written and
/// not yet published; those of one segment, so bounded by a generation.
///
/// `index` is `None` during recovery, which runs before the index opens.
pub(crate) struct Publisher<'a> {
    index: Option<&'a TemporalIndex>,
    shared: &'a Shared,
    segment: Option<PathBuf>,
    pending: Vec<ChunkMeta>,
    /// Chunks were written that the index can no longer be told of until restart,
    /// so the stream stays behind and the tail keeps them readable.
    lost: bool,
}

impl<'a> Publisher<'a> {
    pub(crate) fn new(index: Option<&'a TemporalIndex>, shared: &'a Shared) -> Self {
        Self {
            index,
            shared,
            segment: None,
            pending: Vec::new(),
            lost: false,
        }
    }

    /// Notes one append. A roll sealed the previous segment and fsynced it, so it
    /// is told at once, and that seal covers the metas held for it.
    pub(crate) fn appended(&mut self, appended: Appended) {
        if let Some(summary) = &appended.rolled {
            self.pending.clear();
            self.sealed(summary);
        }
        if self.segment.as_ref() != Some(&appended.segment) {
            self.pending.clear();
            self.segment = Some(appended.segment);
        }
        self.pending.push(appended.chunk);
    }

    /// Publishes what is now on disk. Only ever called after `writer.sync`. An
    /// over-budget tail is reclaimed by sealing the segment.
    pub(crate) fn synced(&mut self, writer: &mut StreamWriter) {
        let Some(index) = self.index else { return };
        let name = self.segment.as_deref().and_then(SegmentName::parse);
        let (Some(name), false) = (name, self.pending.is_empty()) else {
            return;
        };
        let stream = self.shared.stream;
        let result = index.publish_chunks(stream, name, &self.pending);
        self.pending.clear();
        match took_effect(stream, result) {
            Ok(()) => {}
            Err(IndexError::OverBudget { .. }) => match writer.roll() {
                Ok(Some(summary)) => self.sealed(&summary),
                Ok(None) => {}
                Err(e) => {
                    tracing::error!(error = %e, "cannot seal to reclaim index room");
                    self.lost = true;
                    self.shared.index_behind.store(true, Ordering::Release);
                }
            },
            Err(e) => self.behind(&e),
        }
    }

    /// Reports a sealed segment.
    pub(crate) fn sealed(&mut self, summary: &SegmentSummary) {
        let Some(index) = self.index else { return };
        let stream = self.shared.stream;
        if let Err(e) = took_effect(stream, index.segment_sealed(stream, summary)) {
            self.behind(&e);
        }
    }

    fn behind(&self, e: &IndexError) {
        tracing::warn!(stream = self.shared.stream.get(), error = %e, "index behind");
        self.shared.index_behind.store(true, Ordering::Release);
    }

    /// While behind, tries to catch up; success lifts the backpressure.
    pub(crate) fn retry(&mut self) {
        let Some(index) = self.index else { return };
        if self.lost || !self.shared.index_behind.load(Ordering::Acquire) {
            return;
        }
        let stream = self.shared.stream;
        match index.refresh(stream) {
            Ok(_) | Err(IndexError::PersistFailed(_)) => {
                self.shared.index_behind.store(false, Ordering::Release);
            }
            Err(e) => tracing::debug!(error = %e, "index still behind"),
        }
    }
}

impl Ingest {
    /// Tells the index a compaction finished. Whatever runs `compact` calls this
    /// next; `PersistFailed` is logged here. Blocks on the disk.
    ///
    /// # Errors
    /// Any other [`IndexError`]; a caller may then `refresh` and retry. Data is
    /// safe either way.
    pub fn compaction_applied(
        &self,
        stream: StreamId,
        outcome: &CompactionOutcome,
    ) -> Result<(), IndexError> {
        match self.ctx.index.get() {
            Some(index) => took_effect(stream, index.compaction_applied(stream, outcome)),
            None => Ok(()),
        }
    }
}
