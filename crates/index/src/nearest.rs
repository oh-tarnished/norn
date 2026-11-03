//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Timestamp → nearest chunk at or before.

use norn_core::{SignalId, Timestamp};
use norn_storage::format::frame::ChunkMeta;

use crate::error::Result;
use crate::locator::ChunkLocator;
use crate::plan::SignalFilter;
use crate::plan::scan::chunks_of;
use crate::signals::SignalSpan;
use crate::snapshot::Snapshot;
use crate::view::StreamView;

impl Snapshot {
    /// The chunk of `signal` whose samples come closest to `t` without passing
    /// it, over every stream: among chunks with `min_ts <= t`, the greatest
    /// `min(max_ts, t)`, then the latest `min_ts`, then the plan's order. When
    /// several chunks end at the signal's very last sample and `t` is at or after
    /// it, any of them may be returned, from one segment.
    /// `None` when no chunk starts at or before `t`.
    ///
    /// The chunk always holds a sample at or before `t`. Under overlap it may
    /// not hold the latest: read it, take the last sample `s <= t`, then plan
    /// `signal` over `s + 1..=t`, which is empty unless chunks overlap
    /// (docs/reference/storage.md "Nearest at or before").
    ///
    /// Cost: an unknown signal, or a `t` before its first sample, reads nothing.
    /// A `t` at or after its last sample reads one segment, however long it has
    /// been silent ([`Snapshot::last_seen`]). Otherwise it walks segments
    /// backwards from `t` and stops as soon as the running maximum of `max_ts`
    /// falls below the best found, usually after one or two; a `t` inside a long
    /// gap in the signal's history still walks back across the gap.
    ///
    /// # Errors
    /// [`crate::IndexError::SegmentGone`]: refresh and ask again.
    pub fn nearest_at_or_before(
        &self,
        signal: SignalId,
        t: Timestamp,
    ) -> Result<Option<ChunkLocator>> {
        let mut best: Option<ChunkLocator> = None;
        for view in self.streams.values() {
            self.nearest_in(view, signal, t, &mut best)?;
        }
        Ok(best)
    }

    fn nearest_in(
        &self,
        view: &StreamView,
        signal: SignalId,
        t: Timestamp,
        best: &mut Option<ChunkLocator>,
    ) -> Result<()> {
        let Some(span) = view.signals.get(signal).filter(|s| s.first <= t) else {
            return Ok(());
        };
        let stream = view.stream;
        let locate = |segment, sealed, chunk: &ChunkMeta| ChunkLocator {
            stream,
            segment,
            sealed,
            chunk: *chunk,
        };
        if t >= span.last {
            return self.last_chunk(view, signal, span, t, best);
        }
        for tail in view.tails.iter() {
            tail.for_each(Timestamp::MIN, t, |c| {
                if c.signal == signal {
                    offer(best, locate(tail.segment, false, c), t);
                }
            });
        }
        let filter = SignalFilter::one(signal);
        let list = &view.segments;
        let end = list.records.partition_point(|r| r.min_ts <= t);
        let walk = list.records.iter().zip(list.reach.iter()).take(end).rev();
        for (record, reach) in walk {
            // Nothing from here back ends after `reach`. A tie could still win
            // on `min_ts`, so stop only when strictly below the best.
            if best.is_some_and(|b| (*reach).min(t) < b.max_ts().min(t)) {
                break;
            }
            chunks_of(
                &self.shared,
                stream,
                record,
                &filter,
                Timestamp::MIN,
                t,
                |c| offer(best, locate(record.name(), true, c), t),
            )?;
        }
        Ok(())
    }

    /// The chunk ending at `span.last`, from the one segment the summary names.
    fn last_chunk(
        &self,
        view: &StreamView,
        signal: SignalId,
        span: SignalSpan,
        t: Timestamp,
        best: &mut Option<ChunkLocator>,
    ) -> Result<()> {
        let (stream, segment) = (view.stream, span.last_segment);
        let locate = |sealed, chunk: &ChunkMeta| ChunkLocator {
            stream,
            segment,
            sealed,
            chunk: *chunk,
        };
        if let Some(tail) = view.tails.iter().find(|tail| tail.segment == segment) {
            tail.for_each(span.last, t, |c| {
                if c.signal == signal {
                    offer(best, locate(false, c), t);
                }
            });
            return Ok(());
        }
        let list = &view.segments;
        let window = list.window(span.last, span.last);
        let found = list.records.get(window).into_iter().flatten();
        let filter = SignalFilter::one(signal);
        if let Some(record) = found.into_iter().find(|r| r.name() == segment) {
            let (from, shared) = (span.last, &self.shared);
            chunks_of(shared, stream, record, &filter, from, t, |c| {
                offer(best, locate(true, c), t);
            })?;
        }
        Ok(())
    }
}

/// Keeps whichever of `best` and `candidate` ranks higher.
fn offer(best: &mut Option<ChunkLocator>, candidate: ChunkLocator, t: Timestamp) {
    let rank = |c: &ChunkLocator| (c.max_ts().min(t), c.min_ts(), *c);
    if best.as_ref().is_none_or(|b| rank(b) < rank(&candidate)) {
        *best = Some(candidate);
    }
}
