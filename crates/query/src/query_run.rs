//! One running query: rows become pages.

use std::sync::Arc;

use norn_core::{Sample, Timestamp};

use crate::aggregate::{Aggregate, Reduce};
use crate::engine::QueryStats;
use crate::error::Result;
use crate::latest::latest;
use crate::memory::Charge;
use crate::page::{Filling, Page, Results};
use crate::rows::ROW_BYTES;
use crate::token::Position;
use crate::walk::Walk;

/// How rows become results.
#[derive(Debug)]
pub(crate) enum Shape {
    Raw,
    Windows(Box<Reduce>),
    Latest { next: usize },
}

/// One running query, pulled with [`Query::next_page`].
///
/// A pull model on purpose: between pages it holds no thread, only its charges, so
/// a consumer that stops reading costs a little memory rather than a worker.
///
/// Lifecycle: dropping it at any point is the whole of cancelling it, and returns
/// everything to the budget. Concurrency: `Send`, one caller at a time.
#[derive(Debug)]
pub struct Query {
    walk: Walk,
    shape: Shape,
    range: (Timestamp, Timestamp),
    finished: bool,
    _base: Charge,
}

impl Query {
    pub(crate) fn new(
        walk: Walk,
        shape: Shape,
        range: (Timestamp, Timestamp),
        base: Charge,
    ) -> Self {
        Self {
            walk,
            shape,
            range,
            finished: false,
            _base: base,
        }
    }

    /// What this query has done so far.
    #[must_use]
    pub fn stats(&self) -> QueryStats {
        let reader = &self.walk.reader;
        QueryStats {
            chunks_read: reader.chunks_read,
            bytes_read: reader.files.bytes_read(),
            races: reader.races,
        }
    }

    /// The next page, or `None` at the end. Blocks on the disk.
    ///
    /// An error ends the query, and what came before it stands: a slice is read
    /// whole before any of it is delivered, so nothing already delivered is wrong.
    ///
    /// # Errors
    /// [`crate::QueryError`]; [`crate::QueryError::Corrupt`] for damaged data.
    pub fn next_page(&mut self) -> Result<Option<Page>> {
        if self.finished {
            return Ok(None);
        }
        let page = match &self.shape {
            Shape::Raw => self.raw_page(),
            Shape::Windows(_) => self.window_page(),
            Shape::Latest { .. } => self.latest_page(),
        };
        if !matches!(page, Ok(Some(_))) {
            self.finished = true;
        }
        page
    }

    /// Finishes a page, charging it until it is delivered and dropped. `None`
    /// when it is empty: the query is over.
    fn seal<T>(&self, page: Filling<T>, wrap: fn(Vec<T>) -> Results) -> Result<Option<Page>> {
        let Some(position) = page.position else {
            return Ok(None);
        };
        let mut charge = Charge::new(Arc::clone(&self.walk.reader.engine.memory));
        charge.grow(page.bytes)?;
        Ok(Some(Page {
            results: wrap(page.items),
            position,
            charge,
        }))
    }

    fn raw_page(&mut self) -> Result<Option<Page>> {
        let limits = self.walk.reader.engine.limits;
        let mut page: Filling<(Arc<str>, Sample)> = Filling::new();
        while !page.full(&limits) {
            let Some(row) = self.walk.next()? else { break };
            let Some(name) = self.walk.name_of(row.target) else {
                continue;
            };
            let position = Position::Sample {
                signal: name.to_string(),
                time: row.sample.timestamp,
                sequence: row.sample.sequence.get(),
            };
            let bytes = ROW_BYTES + row.payload() + name.len() as u64;
            page.add((name, row.sample), position, bytes);
        }
        self.seal(page, Results::Samples)
    }

    fn window_page(&mut self) -> Result<Option<Page>> {
        let limits = self.walk.reader.engine.limits;
        let mut page: Filling<Aggregate> = Filling::new();
        let Shape::Windows(reduce) = &mut self.shape else {
            return Ok(None);
        };
        while !page.full(&limits) {
            let row = self.walk.next()?;
            let closed = match &row {
                Some(row) => reduce.add(row, self.walk.targets()),
                None => reduce.finish(self.walk.targets()),
            };
            if let Some((index, aggregate)) = closed {
                let signal = aggregate.signal.to_string();
                let bytes = ROW_BYTES + signal.len() as u64;
                page.add(aggregate, Position::Window { signal, index }, bytes);
            }
            if row.is_none() {
                break;
            }
        }
        self.seal(page, Results::Aggregates)
    }

    fn latest_page(&mut self) -> Result<Option<Page>> {
        let limits = self.walk.reader.engine.limits;
        let mut page: Filling<Aggregate> = Filling::new();
        let Shape::Latest { next } = &mut self.shape else {
            return Ok(None);
        };
        let (filter, targets) = self.walk.parts();
        while !page.full(&limits) {
            let Some((index, target)) = targets.get(*next).cloned() else {
                break;
            };
            *next += 1;
            let found = latest(&mut self.walk.reader, index, &target, self.range, &filter)?;
            let Some(sample) = found else { continue };
            let aggregate = Aggregate {
                signal: Arc::clone(&target.name),
                start: Some(sample.timestamp),
                end: sample
                    .timestamp
                    .as_nanos()
                    .checked_add(1)
                    .map(Timestamp::from_nanos),
                value: sample.value,
                samples: 1,
            };
            let signal = target.name.to_string();
            let bytes = ROW_BYTES + signal.len() as u64;
            page.add(aggregate, Position::Window { signal, index: 0 }, bytes);
        }
        self.seal(page, Results::Aggregates)
    }
}
