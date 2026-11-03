//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Walking a query's rows in order, slice by slice, in bounded memory.

use std::sync::Arc;

use norn_core::Timestamp;

use crate::error::{QueryError, Result};
use crate::filter::Filter;
use crate::memory::Charge;
use crate::read::{Reader, Stop};
use crate::resume::{After, Start};
use crate::rows::Row;
use crate::selection::Selection;
use crate::slice::Slicer;
use crate::spec::Target;

/// One signal in `BySignal` or aggregate order, or all of them merged.
#[derive(Debug)]
struct Group {
    selection: Selection,
    cursor: Option<Timestamp>,
    slicer: Slicer,
}

/// The rows of a query, group after group, in (time, sequence, target) order
/// within a group.
///
/// Holds one slice's rows at a time. Concurrency: owned by one query.
#[derive(Debug)]
pub(crate) struct Walk {
    pub(crate) reader: Reader,
    targets: Vec<(u32, Arc<Target>)>,
    merged: bool,
    next_group: usize,
    group: Option<Group>,
    pending: std::vec::IntoIter<Row>,
    held: Option<Charge>,
    from: Timestamp,
    to: Timestamp,
    first_from: Option<Timestamp>,
    after: Option<After>,
    filter: Filter,
}

impl Walk {
    pub(crate) fn new(
        reader: Reader,
        targets: Vec<(u32, Arc<Target>)>,
        merged: bool,
        range: (Timestamp, Timestamp),
        filter: Filter,
        start: Start,
    ) -> Self {
        Self {
            reader,
            targets,
            merged,
            next_group: start.skip,
            group: None,
            pending: Vec::new().into_iter(),
            held: None,
            from: range.0,
            to: range.1,
            first_from: start.from,
            after: start.after,
            filter,
        }
    }

    /// The name of target `index`, sharing it.
    pub(crate) fn name_of(&self, index: u32) -> Option<Arc<str>> {
        let target = self.targets.get(index as usize)?;
        Some(Arc::clone(&target.1.name))
    }

    /// Every target.
    pub(crate) fn targets(&self) -> &[(u32, Arc<Target>)] {
        &self.targets
    }

    /// Copies of the filter and the target list, for LATEST, which walks by itself.
    pub(crate) fn parts(&self) -> (Filter, Vec<(u32, Arc<Target>)>) {
        (self.filter.clone(), self.targets.clone())
    }

    /// The next row, or `None` at the end.
    pub(crate) fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if let Some(row) = self.pending.next() {
                return Ok(Some(row));
            }
            self.held = None;
            if !self.next_slice()? {
                return Ok(None);
            }
        }
    }

    /// Starts the next group; false when there are none.
    fn next_group(&mut self) -> bool {
        let chosen: Vec<_> = if self.merged {
            if self.next_group > 0 {
                return false;
            }
            self.next_group = 1;
            self.targets.clone()
        } else {
            let Some(target) = self.targets.get(self.next_group) else {
                return false;
            };
            self.next_group += 1;
            vec![target.clone()]
        };
        let cursor = self.first_from.take().unwrap_or(self.from);
        self.group = Some(Group {
            selection: Selection::new(chosen),
            cursor: Some(cursor),
            slicer: Slicer::new(cursor, self.to),
        });
        true
    }

    /// Reads one slice into `pending`; false when the walk is over.
    fn next_slice(&mut self) -> Result<bool> {
        loop {
            if self.group.is_none() && !self.next_group() {
                return Ok(false);
            }
            let Some(mut group) = self.group.take() else {
                return Ok(false);
            };
            let Some(cursor) = group.cursor.filter(|c| *c <= self.to) else {
                continue;
            };
            let read = self.read_slice(&mut group, cursor)?;
            self.group = Some(group);
            if read {
                return Ok(true);
            }
        }
    }

    /// One attempt at a slice from `cursor`. False when it must be narrowed.
    fn read_slice(&mut self, group: &mut Group, cursor: Timestamp) -> Result<bool> {
        let limit = self.reader.engine.limits.slice_bytes;
        let engine = Arc::clone(&self.reader.engine);
        let end = group
            .slicer
            .end(&engine, &group.selection, cursor, self.to, limit);
        // One instant cannot be cut further, so the budget alone bounds it.
        let cap = if end == cursor { u64::MAX } else { limit };
        let read = self
            .reader
            .read(&group.selection, cursor, end, &self.filter, cap);
        match read {
            Ok((mut rows, charge)) => {
                group.slicer.widen(charge.bytes(), limit);
                group.cursor = (end < self.to)
                    .then(|| Timestamp::from_nanos(end.as_nanos().saturating_add(1)));
                if let Some(after) = self.after {
                    rows.retain(|row| !after.delivered(row));
                    if end >= after.time {
                        self.after = None;
                    }
                }
                self.pending = rows.into_iter();
                self.held = Some(charge);
                Ok(true)
            }
            Err(Stop::Failed(e)) => Err(e),
            Err(Stop::Narrow) if end == cursor => Err(QueryError::TooLarge),
            Err(Stop::Memory(e)) if end == cursor => Err(e.into()),
            Err(Stop::Narrow | Stop::Memory(_)) => {
                group.slicer.narrow(cursor, end);
                Ok(false)
            }
        }
    }
}
