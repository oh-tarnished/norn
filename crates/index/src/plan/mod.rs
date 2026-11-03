//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The plan: a lazy, time-ordered merge of candidate chunks.

mod load;
mod request;
pub(crate) mod scan;

pub use request::{PlanRequest, SignalFilter};

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ops::Range;
use std::sync::Arc;

use norn_core::Timestamp;

use crate::error::Result;
use crate::locator::ChunkLocator;
use crate::snapshot::Snapshot;
use crate::tail::TailLog;
use crate::view::StreamView;

/// One stream's unread segments.
#[derive(Debug)]
struct Cursor {
    view: Arc<StreamView>,
    window: Range<usize>,
}

/// A candidate source, identified for loading.
#[derive(Debug, Clone, Copy)]
enum Source {
    Segment { cursor: usize },
    Tail { index: usize },
}

/// Streams the chunks a [`PlanRequest`] matches, in the total order of
/// [`ChunkLocator`], so ascending by `min_ts`.
///
/// Items are `Result`: after an `Err` the iterator ends. Resume by planning from
/// the last timestamp delivered, per signal.
///
/// A segment is loaded only once nothing earlier can come out, so a plan that is
/// dropped early never touches the rest, and no segment outside the window, the
/// time range or the signal range is touched at all. It reads sidecars only;
/// segment files are the reader's business.
///
/// Memory: the chunks of segments overlapping in time at that moment, at most
/// [`crate::IndexOptions::max_plan_buffered`], past which it yields
/// [`crate::IndexError::PlanTooLarge`] rather than allocate. A segment holding a year
/// of late data makes everything overlap, and is what that bound is for.
///
/// Concurrency: `Send`; owns its snapshot, so it sees one consistent state however
/// long it lives.
#[derive(Debug)]
pub struct Plan {
    snapshot: Arc<Snapshot>,
    request: PlanRequest,
    cursors: Vec<Cursor>,
    tails: Vec<(Reverse<Timestamp>, Arc<TailLog>, norn_core::StreamId)>,
    heap: BinaryHeap<Reverse<ChunkLocator>>,
    done: bool,
}

impl Plan {
    pub(crate) fn new(snapshot: Arc<Snapshot>, request: PlanRequest) -> Self {
        let (from, to) = (request.from, request.to);
        let mut cursors = Vec::new();
        let mut tails = Vec::new();
        let wanted = |s: &norn_core::StreamId| request.stream.is_none_or(|r| r == *s);
        if from <= to {
            for view in snapshot.streams.iter().filter(|(s, _)| wanted(s)) {
                let view = Arc::clone(view.1);
                for tail in view.tails.iter() {
                    if let Some((min, _)) = tail.range().filter(|(a, b)| *a <= to && *b >= from) {
                        tails.push((Reverse(min), Arc::clone(tail), view.stream));
                    }
                }
                let window = view.segments.window(from, to);
                cursors.push(Cursor { view, window });
            }
        }
        // Latest first, so the earliest is popped from the end.
        tails.sort_by_key(|t| t.0);
        Self {
            snapshot,
            request,
            cursors,
            tails,
            heap: BinaryHeap::new(),
            done: false,
        }
    }

    /// The earliest source not yet loaded. Skips, unread, segments that cannot
    /// match. O(streams).
    fn next_source(&mut self) -> Option<(Timestamp, Source)> {
        let (from, to) = (self.request.from, self.request.to);
        let filter = &self.request.signals;
        let mut best = self.tails.last().map(|t| {
            (
                t.0.0,
                Source::Tail {
                    index: self.tails.len() - 1,
                },
            )
        });
        for (cursor, c) in self.cursors.iter_mut().enumerate() {
            let records = &c.view.segments.records;
            while let Some(r) = c.window.clone().next().and_then(|i| records.get(i)) {
                if r.overlaps(from, to) && filter.may_hold(r.min_signal, r.max_signal) {
                    if best.is_none_or(|(t, _)| r.min_ts < t) {
                        best = Some((r.min_ts, Source::Segment { cursor }));
                    }
                    break;
                }
                c.window.start += 1;
            }
        }
        best
    }
}

impl Iterator for Plan {
    type Item = Result<ChunkLocator>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            let top = self.heap.peek().map(|Reverse(l)| l.min_ts());
            match (top, self.next_source()) {
                // Ties must be loaded: the locator order decides among them.
                (top, Some((m, source))) if top.is_none_or(|t| m <= t) => {
                    if let Err(e) = self.load(source) {
                        self.done = true;
                        return Some(Err(e));
                    }
                }
                (None, _) => {
                    self.done = true;
                    return None;
                }
                (Some(_), _) => return self.heap.pop().map(|Reverse(l)| Ok(l)),
            }
        }
    }
}
