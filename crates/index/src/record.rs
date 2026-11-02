//! The per-segment summary: what the index keeps in memory for every segment.

use norn_core::{SignalId, Timestamp};
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{SegmentHeader, SegmentName};

use crate::wire::Cursor;

/// Bytes of one encoded record in `summary.nidx`.
pub(crate) const RECORD_LEN: usize = 72;

/// One segment as the index knows it: identity, time range and signal range.
///
/// 72 bytes, no path and no allocation: a year of ingest segments is 35,040 of
/// these. An empty segment has `chunks == 0`, `min_ts == Timestamp::MAX` and
/// `max_ts == Timestamp::MIN`, so it sorts last and overlaps nothing.
///
/// Invariant: a segment is immutable once sealed, so a record never changes;
/// `file_len` ties it to one version of the file.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentRecord {
    /// The first segment sequence number it covers; also in its file name.
    pub first_seq: u64,
    /// The last segment sequence number it covers.
    pub last_seq: u64,
    /// Its compaction generation.
    pub generation: u16,
    /// Number of chunks.
    pub chunks: u32,
    /// Number of samples over all chunks.
    pub samples: u64,
    /// File size in bytes.
    pub file_len: u64,
    /// The earliest sample timestamp.
    pub min_ts: Timestamp,
    /// The latest sample timestamp.
    pub max_ts: Timestamp,
    /// The lowest signal id held.
    pub min_signal: SignalId,
    /// The highest signal id held.
    pub max_signal: SignalId,
}

impl SegmentRecord {
    /// Summarises a sealed segment's directory. O(chunks); reads nothing.
    pub(crate) fn of(header: &SegmentHeader, chunks: &[ChunkMeta], file_len: u64) -> Self {
        let mut r = Self {
            first_seq: header.first_seq,
            last_seq: header.last_seq,
            generation: header.generation,
            chunks: u32::try_from(chunks.len()).unwrap_or(u32::MAX),
            samples: 0,
            file_len,
            min_ts: Timestamp::MAX,
            max_ts: Timestamp::MIN,
            min_signal: SignalId::new(u64::MAX),
            max_signal: SignalId::new(0),
        };
        for c in chunks {
            r.samples = r.samples.saturating_add(u64::from(c.count));
            r.min_ts = r.min_ts.min(c.min_ts);
            r.max_ts = r.max_ts.max(c.max_ts);
            r.min_signal = r.min_signal.min(c.signal);
            r.max_signal = r.max_signal.max(c.signal);
        }
        r
    }

    /// The name its file has.
    #[must_use]
    pub fn name(&self) -> SegmentName {
        SegmentName {
            seq: self.first_seq,
            generation: self.generation,
        }
    }

    /// Whether any chunk may hold a sample in `from..=to`.
    #[must_use]
    pub fn overlaps(&self, from: Timestamp, to: Timestamp) -> bool {
        self.chunks > 0 && self.min_ts <= to && self.max_ts >= from
    }

    /// Whether `self` holds every sample of `other`: the same predicate as
    /// `norn_storage::SegmentEntry::supersedes`.
    #[must_use]
    pub fn supersedes(&self, other: &Self) -> bool {
        self.generation > other.generation
            && self.first_seq <= other.first_seq
            && other.last_seq <= self.last_seq
    }

    /// Appends the [`RECORD_LEN`] encoded bytes.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.first_seq.to_le_bytes());
        out.extend_from_slice(&self.last_seq.to_le_bytes());
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.chunks.to_le_bytes());
        out.extend_from_slice(&self.samples.to_le_bytes());
        out.extend_from_slice(&self.file_len.to_le_bytes());
        out.extend_from_slice(&self.min_ts.as_nanos().to_le_bytes());
        out.extend_from_slice(&self.max_ts.as_nanos().to_le_bytes());
        out.extend_from_slice(&self.min_signal.get().to_le_bytes());
        out.extend_from_slice(&self.max_signal.get().to_le_bytes());
    }

    /// Reads one record; `None` when `r` runs out. Copies.
    pub(crate) fn decode(r: &mut Cursor<'_>) -> Option<Self> {
        let first_seq = r.u64()?;
        let last_seq = r.u64()?;
        let generation = r.u16()?;
        let _reserved = r.u16()?;
        Some(Self {
            first_seq,
            last_seq,
            generation,
            chunks: r.u32()?,
            samples: r.u64()?,
            file_len: r.u64()?,
            min_ts: Timestamp::from_nanos(r.i64()?),
            max_ts: Timestamp::from_nanos(r.i64()?),
            min_signal: SignalId::new(r.u64()?),
            max_signal: SignalId::new(r.u64()?),
        })
    }
}
