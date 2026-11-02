//! One stream's immutable view: its segment list and its tails.

use std::ops::Range;
use std::sync::Arc;

use norn_core::{StreamId, Timestamp};
use norn_storage::SegmentName;

use crate::live::retain_live;
use crate::record::SegmentRecord;
use crate::signals::SignalMap;
use crate::tail::TailLog;

/// The global tier for one stream: records sorted by time, plus a running
/// maximum that makes overlapping ranges searchable.
///
/// Invariant: `records` sorts by `(min_ts, first_seq, generation)`, and
/// `reach[i]` is the largest `max_ts` among `records[..=i]`, so it never
/// decreases and a binary search on it is sound.
///
/// Memory: 80 bytes a segment.
#[derive(Debug, Default)]
pub(crate) struct SegmentList {
    pub(crate) records: Vec<SegmentRecord>,
    pub(crate) reach: Vec<Timestamp>,
}

impl SegmentList {
    /// A list of `records`, sorted here.
    pub(crate) fn new(mut records: Vec<SegmentRecord>) -> Self {
        records.sort_by_key(|r| (r.min_ts, r.first_seq, r.generation));
        let mut max = Timestamp::MIN;
        let reach = records
            .iter()
            .map(|r| {
                max = max.max(r.max_ts);
                max
            })
            .collect();
        Self { records, reach }
    }

    /// The indices that can overlap `from..=to`. O(log n). Not every record in
    /// it does: under overlap, some must still be checked one by one.
    pub(crate) fn window(&self, from: Timestamp, to: Timestamp) -> Range<usize> {
        let start = self.reach.partition_point(|m| *m < from);
        let end = self.records.partition_point(|r| r.min_ts <= to);
        start..end.max(start)
    }

    fn memory_bytes(&self) -> u64 {
        (self.records.len() * (size_of::<SegmentRecord>() + size_of::<Timestamp>())) as u64
    }
}

/// A stream as one snapshot sees it. Immutable; updates build a new one.
#[derive(Debug)]
pub(crate) struct StreamView {
    pub(crate) stream: StreamId,
    pub(crate) segments: Arc<SegmentList>,
    pub(crate) tails: Arc<[Arc<TailLog>]>,
    pub(crate) signals: Arc<SignalMap>,
}

impl StreamView {
    /// A view of the live subset of `records`, with `tails`.
    pub(crate) fn new(
        stream: StreamId,
        mut records: Vec<SegmentRecord>,
        tails: Vec<Arc<TailLog>>,
    ) -> Self {
        retain_live(&mut records);
        Self {
            stream,
            segments: Arc::new(SegmentList::new(records)),
            tails: Arc::from(tails),
            signals: Arc::default(),
        }
    }

    /// This view with `signals` as its per-signal summary.
    pub(crate) fn with_signals(mut self, signals: Arc<SignalMap>) -> Self {
        self.signals = signals;
        self
    }

    /// This view with `add` added (replacing a record of the same name), keeping
    /// only live segments, so a compaction output removes its inputs. A tail
    /// whose segment is `add` goes in the same step, so its chunks are never
    /// missing and never answered twice. Applying it twice changes nothing.
    pub(crate) fn with_record(&self, add: SegmentRecord) -> Self {
        let mut records: Vec<_> = self
            .segments
            .records
            .iter()
            .filter(|r| r.name() != add.name())
            .copied()
            .collect();
        records.push(add);
        let tails = self
            .tails
            .iter()
            .filter(|t| t.segment != add.name())
            .cloned()
            .collect();
        Self::new(self.stream, records, tails).with_signals(Arc::clone(&self.signals))
    }

    /// This view with `tail` replacing the tail of its segment.
    pub(crate) fn with_tail(&self, tail: TailLog) -> Self {
        let mut tails: Vec<_> = self
            .tails
            .iter()
            .filter(|t| t.segment != tail.segment)
            .cloned()
            .collect();
        tails.push(Arc::new(tail));
        Self {
            stream: self.stream,
            segments: Arc::clone(&self.segments),
            tails: Arc::from(tails),
            signals: Arc::clone(&self.signals),
        }
    }

    /// Whether a sealed record already holds `segment`'s chunks: that segment
    /// itself, or a compaction output covering its sequence number.
    pub(crate) fn covers(&self, segment: SegmentName) -> bool {
        let seq = segment.seq;
        self.segments
            .records
            .iter()
            .any(|r| r.first_seq <= seq && seq <= r.last_seq)
    }

    /// Bytes charged to the budget: the list, every tail and the signal map.
    pub(crate) fn memory_bytes(&self) -> u64 {
        let tails: u64 = self.tails.iter().map(|t| t.memory_bytes()).sum();
        self.segments.memory_bytes()
            + tails
            + self.signals.memory_bytes()
            + size_of::<Self>() as u64
    }
}
