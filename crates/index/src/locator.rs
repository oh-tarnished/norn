//! Where a chunk is: what a plan yields and a reader consumes.

use std::cmp::Ordering;

use norn_core::{StreamId, Timestamp};
use norn_storage::SegmentName;
use norn_storage::format::frame::ChunkMeta;

/// A chunk's stream, segment and metadata: enough to read it with
/// `norn_storage::ChunkFile::read_chunk(&locator.chunk)`, after
/// [`crate::Snapshot::path_of`] gives the file.
///
/// Locators are valid for the snapshot that made them. Compaction changes chunk
/// boundaries, so one must never be carried over to a new plan.
///
/// Ordering: the plan's total order `(min_ts, stream, first_seq, generation,
/// offset)`. Two chunks never compare equal unless they are the same chunk.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLocator {
    /// The stream.
    pub stream: StreamId,
    /// The segment file's identity.
    pub segment: SegmentName,
    /// Whether the segment was sealed in this snapshot. An unsealed one may
    /// seal or be compacted later, under the same name.
    pub sealed: bool,
    /// Offset, body length, signal, kind, time range and count.
    pub chunk: ChunkMeta,
}

impl ChunkLocator {
    /// Byte offset of the chunk's frame within the segment file.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.chunk.offset
    }

    /// The earliest sample timestamp.
    #[must_use]
    pub fn min_ts(&self) -> Timestamp {
        self.chunk.min_ts
    }

    /// The latest sample timestamp.
    #[must_use]
    pub fn max_ts(&self) -> Timestamp {
        self.chunk.max_ts
    }

    fn key(&self) -> (Timestamp, StreamId, SegmentName, u64) {
        (
            self.chunk.min_ts,
            self.stream,
            self.segment,
            self.chunk.offset,
        )
    }
}

impl Ord for ChunkLocator {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for ChunkLocator {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
