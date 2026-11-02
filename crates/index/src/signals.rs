//! The per-signal summary: where each signal starts and when it was last seen.
//!
//! `nearest_at_or_before` used to walk every segment back from `t` for a signal
//! that had gone silent, and every segment there is for one that never existed:
//! 391 ms over a compacted year. This map answers both from memory.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use norn_core::{SignalId, Timestamp};
use norn_storage::SegmentName;
use norn_storage::format::frame::ChunkMeta;

use crate::error::Result;
use crate::plan::SignalFilter;
use crate::plan::scan::chunks_of;
use crate::record::SegmentRecord;
use crate::shared::Shared;
use crate::tail::TailLog;
use crate::view::StreamView;

/// What the index keeps about one signal: 48 bytes, whatever its history.
///
/// `first` is exact until retention removes the signal's oldest chunks, and a
/// lower bound after that. `last` and `last_segment` are exact for the snapshot
/// holding them: a chunk of `last_segment` ends at `last`, and nothing of this
/// signal ends later.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalSpan {
    /// No sample of the signal is earlier than this.
    pub first: Timestamp,
    /// The latest sample's timestamp.
    pub last: Timestamp,
    /// A segment holding a chunk that ends at `last`.
    pub last_segment: SegmentName,
}

/// Every signal of a stream with a chunk the index knows, sorted by id.
///
/// Immutable: an update builds a new one, so a snapshot keeps what it saw. A
/// flat vector, because cloning one is a `memcpy` of the kind a publish can
/// afford, and a binary search needs nothing else.
///
/// Memory: 56 bytes a signal, charged to the index budget, so at most
/// `max_signals` entries: about 5.6 MiB at the default limit.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SignalMap {
    pub(crate) entries: Vec<(SignalId, SignalSpan)>,
}

impl SignalMap {
    /// The span of `signal`. O(log n).
    pub(crate) fn get(&self, signal: SignalId) -> Option<SignalSpan> {
        let found = self.entries.binary_search_by_key(&signal, |e| e.0);
        found.ok().and_then(|i| self.entries.get(i)).map(|e| e.1)
    }

    /// This map with `chunks` of `segment` added. A chunk ending at or after a
    /// signal's `last` takes it over, which is what moves a signal off a
    /// compaction's inputs. O(chunks × log n), plus a copy of the map.
    pub(crate) fn absorbed<'a>(
        &self,
        segment: SegmentName,
        chunks: impl IntoIterator<Item = &'a ChunkMeta>,
    ) -> Self {
        let mut entries = self.entries.clone();
        let mut fresh: BTreeMap<SignalId, SignalSpan> = BTreeMap::new();
        for c in chunks {
            let found = entries.binary_search_by_key(&c.signal, |e| e.0);
            let span = found
                .ok()
                .and_then(|i| entries.get_mut(i))
                .map(|e| &mut e.1);
            match span.or_else(|| fresh.get_mut(&c.signal)) {
                Some(span) => widen(span, segment, c),
                None => {
                    let span = SignalSpan {
                        first: c.min_ts,
                        last: c.max_ts,
                        last_segment: segment,
                    };
                    fresh.insert(c.signal, span);
                }
            }
        }
        if !fresh.is_empty() {
            entries.extend(fresh);
            entries.sort_unstable_by_key(|e| e.0);
        }
        Self { entries }
    }

    /// Whether every signal's `last_segment` is live in `view`. False only when
    /// retention removed the chunk a signal was last seen in.
    pub(crate) fn all_live(&self, view: &StreamView) -> bool {
        let records = view.segments.records.iter().map(SegmentRecord::name);
        let tails = view.tails.iter().map(|t| t.segment);
        let live: HashSet<SegmentName> = records.chain(tails).collect();
        self.entries
            .iter()
            .all(|e| live.contains(&e.1.last_segment))
    }

    /// Bytes charged to the budget.
    pub(crate) fn memory_bytes(&self) -> u64 {
        (self.entries.len() * size_of::<(SignalId, SignalSpan)>()) as u64
    }
}

fn widen(span: &mut SignalSpan, segment: SegmentName, c: &ChunkMeta) {
    span.first = span.first.min(c.min_ts);
    if c.max_ts >= span.last {
        span.last = c.max_ts;
        span.last_segment = segment;
    }
}

/// `map` with every chunk of the sealed `record`, read from its sidecar.
///
/// # Errors
/// [`crate::IndexError::SegmentGone`].
pub(crate) fn absorb_record(
    shared: &Shared,
    view: &StreamView,
    map: &SignalMap,
    record: &SegmentRecord,
) -> Result<SignalMap> {
    let mut chunks = Vec::new();
    let all = SignalFilter::all();
    let (from, to) = (Timestamp::MIN, Timestamp::MAX);
    chunks_of(shared, view.stream, record, &all, from, to, |c| {
        chunks.push(*c);
    })?;
    Ok(map.absorbed(record.name(), &chunks))
}

/// `map` with every chunk of `tail`.
pub(crate) fn absorb_tail(map: &SignalMap, tail: &TailLog) -> SignalMap {
    let mut chunks = Vec::with_capacity(tail.len());
    tail.for_each(Timestamp::MIN, Timestamp::MAX, |c| chunks.push(*c));
    map.absorbed(tail.segment, &chunks)
}

/// The map of `view`, from nothing but its sidecars and tails. Reads every page
/// of every segment, in `(min_ts, ...)` order, keeping one segment's metas at a
/// time. Blocks on the disk, in time proportional to all chunks; startup and
/// retention are the only callers, and startup verifies far more than this.
///
/// # Errors
/// [`crate::IndexError::SegmentGone`].
pub(crate) fn rebuild(shared: &Shared, view: &StreamView) -> Result<Arc<SignalMap>> {
    let mut map = SignalMap::default();
    for record in &view.segments.records {
        map = absorb_record(shared, view, &map, record)?;
    }
    for tail in view.tails.iter() {
        map = absorb_tail(&map, tail);
    }
    Ok(Arc::new(map))
}
