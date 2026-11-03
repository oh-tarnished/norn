//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a session's samples in order, a page at a time, resumably.

use std::collections::VecDeque;
use std::sync::Arc;

use norn_core::{Sample, Timestamp};
use norn_query::{
    Charge, Engine, Filter, Order, Output, Position, Query, QuerySpec, Results, Target, TimeRange,
};

use crate::error::ReplayError;
use crate::event::Hold;

/// A position in the replay order, read through `norn-query`.
///
/// Its order is the query's `ByTime`: (time, sequence, signal name). It re-plans
/// every slice from the index and remembers a position rather than a chunk, so
/// compaction under it costs a re-read and no event.
pub(crate) struct Cursor {
    engine: Arc<Engine>,
    targets: Vec<Target>,
    from: Timestamp,
    end: Timestamp,
    after: Option<Position>,
    query: Option<Query>,
    page: VecDeque<(Arc<str>, Sample)>,
    hold: Option<Hold>,
    exhausted: bool,
}

impl std::fmt::Debug for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cursor")
            .field("from", &self.from)
            .field("after", &self.after)
            .field("buffered", &self.page.len())
            .finish_non_exhaustive()
    }
}

impl Cursor {
    pub(crate) fn new(
        engine: Arc<Engine>,
        targets: Vec<Target>,
        from: Timestamp,
        end: Timestamp,
    ) -> Self {
        Self {
            engine,
            targets,
            from,
            end,
            after: None,
            query: None,
            page: VecDeque::new(),
            hold: None,
            exhausted: false,
        }
    }

    pub(crate) fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub(crate) fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// Drops everything read: the next sample is the first at or after `to`.
    pub(crate) fn seek(&mut self, to: Timestamp) {
        self.from = to;
        self.after = None;
        self.query = None;
        self.page.clear();
        self.hold = None;
        self.exhausted = false;
    }

    /// Whether the last page has gone and the query said it was the last.
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted && self.page.is_empty()
    }

    /// The next sample's time, if a page is held.
    pub(crate) fn peek(&self) -> Option<Timestamp> {
        self.page.front().map(|(_, s)| s.timestamp)
    }

    /// What pays for the current page.
    pub(crate) fn hold(&self) -> Option<Hold> {
        self.hold.clone()
    }

    /// The next sample of the held page.
    pub(crate) fn pop(&mut self) -> Option<(Arc<str>, Sample)> {
        let (name, sample) = self.page.pop_front()?;
        self.after = Some(Position::Sample {
            signal: name.to_string(),
            time: sample.timestamp,
            sequence: sample.sequence.get(),
        });
        Some((name, sample))
    }

    fn open(&self) -> Result<Query, ReplayError> {
        let spec = QuerySpec {
            targets: self.targets.clone(),
            range: TimeRange {
                start: Some(self.from),
                end: Some(self.end),
            },
            filter: Filter::default(),
            output: Output::Samples(Order::ByTime),
            resume: self.after.clone(),
        };
        Ok(self.engine.open(spec)?)
    }

    /// Reads the next page. Blocks on the disk. After a failure the query is
    /// reopened from the last sample taken, so calling this again retries.
    pub(crate) fn fill(&mut self) -> Result<(), ReplayError> {
        // The drained page's charge goes before the next is asked for: with a
        // budget of about one page, `next_page` would otherwise wait on memory
        // only this cursor could free, and the replay stalls after one page.
        // Events still on their way hold their own clones of it.
        if self.page.is_empty() {
            self.hold = None;
        }
        let mut query = match self.query.take() {
            Some(query) => query,
            None => self.open()?,
        };
        match query.next_page()? {
            Some(page) => {
                if let Results::Samples(list) = page.results {
                    self.page = list.into();
                }
                let charge: Charge = page.charge;
                self.hold = Some(Arc::new(charge));
                self.query = Some(query);
            }
            None => self.exhausted = true,
        }
        Ok(())
    }
}
