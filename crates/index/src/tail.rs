//! The in-memory tail: chunks written to a segment that is not sealed yet.

use std::sync::Arc;

use norn_core::Timestamp;
use norn_storage::SegmentName;
use norn_storage::format::frame::ChunkMeta;

/// Metas in one frozen block. Picked so that a publish copies at most about
/// 12 KiB, and a time-pruned scan skips whole blocks.
const BLOCK: usize = 256;
/// What one meta costs in memory.
const META_BYTES: usize = size_of::<ChunkMeta>();

/// A frozen run of metas and its time range.
#[derive(Debug, Clone)]
struct Block {
    chunks: Arc<[ChunkMeta]>,
    min_ts: Timestamp,
    max_ts: Timestamp,
}

impl Block {
    fn overlaps(&self, from: Timestamp, to: Timestamp) -> bool {
        self.min_ts <= to && self.max_ts >= from
    }
}

/// An immutable view of one unsealed segment's chunks.
///
/// [`TailLog::appended`] returns a new log sharing every frozen block with the
/// old one, so an old snapshot goes on seeing exactly what it saw, and
/// publishing costs one block's copy rather than the whole tail.
///
/// Memory: 48 bytes a chunk, at most `max_chunks_per_segment` chunks.
#[derive(Debug, Clone)]
pub(crate) struct TailLog {
    pub(crate) segment: SegmentName,
    frozen: Arc<[Block]>,
    open: Arc<[ChunkMeta]>,
    len: usize,
    next_offset: u64,
    range: Option<(Timestamp, Timestamp)>,
}

impl TailLog {
    /// An empty log of `segment`.
    pub(crate) fn new(segment: SegmentName) -> Self {
        Self {
            segment,
            frozen: Arc::from(Vec::new()),
            open: Arc::from(Vec::new()),
            len: 0,
            next_offset: 0,
            range: None,
        }
    }

    /// A log with `metas` appended, in file order. Metas before the end of what
    /// it already holds are ignored, so publishing twice is harmless.
    pub(crate) fn appended(&self, metas: &[ChunkMeta]) -> Self {
        let mut next = self.clone();
        let mut open = self.open.to_vec();
        let mut frozen: Option<Vec<Block>> = None;
        for m in metas.iter().filter(|m| m.offset >= self.next_offset) {
            open.push(*m);
            next.len += 1;
            next.next_offset = m.offset.saturating_add(m.frame_len());
            next.range = Some(match next.range {
                None => (m.min_ts, m.max_ts),
                Some((a, b)) => (a.min(m.min_ts), b.max(m.max_ts)),
            });
            if open.len() == BLOCK {
                frozen
                    .get_or_insert_with(|| self.frozen.to_vec())
                    .push(freeze(&open));
                open.clear();
            }
        }
        if let Some(frozen) = frozen {
            next.frozen = Arc::from(frozen);
        }
        next.open = Arc::from(open);
        next
    }

    /// The earliest and latest timestamp held, if any.
    pub(crate) fn range(&self) -> Option<(Timestamp, Timestamp)> {
        self.range
    }

    /// Chunks held.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Calls `f` for every chunk overlapping `from..=to`, skipping whole blocks
    /// outside it.
    pub(crate) fn for_each(&self, from: Timestamp, to: Timestamp, mut f: impl FnMut(&ChunkMeta)) {
        let frozen = self.frozen.iter().filter(|b| b.overlaps(from, to));
        frozen
            .flat_map(|b| b.chunks.iter())
            .chain(self.open.iter())
            .filter(|c| c.overlaps(from, to))
            .for_each(&mut f);
    }

    /// Bytes this log holds.
    pub(crate) fn memory_bytes(&self) -> u64 {
        let blocks = self.frozen.len() * size_of::<Block>();
        (self.len.saturating_mul(META_BYTES) + blocks + size_of::<Self>()) as u64
    }
}

fn freeze(chunks: &[ChunkMeta]) -> Block {
    let min_ts = chunks.iter().map(|c| c.min_ts).min();
    let max_ts = chunks.iter().map(|c| c.max_ts).max();
    Block {
        chunks: Arc::from(chunks),
        min_ts: min_ts.unwrap_or(Timestamp::MAX),
        max_ts: max_ts.unwrap_or(Timestamp::MIN),
    }
}
