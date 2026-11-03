//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The rows of one read, charged as they grow.

use norn_core::{Sample, Timestamp, Value};
use norn_storage::Chunk;

use crate::filter::Filter;
use crate::memory::Charge;
use crate::read::Stop;

/// What one row costs held.
pub(crate) const ROW_BYTES: u64 = size_of::<Row>() as u64;

/// One sample and the index of its signal among the query's sorted targets.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub(crate) target: u32,
    pub(crate) sample: Sample,
}

impl Row {
    fn key(&self) -> (Timestamp, u64, u32) {
        (
            self.sample.timestamp,
            self.sample.sequence.get(),
            self.target,
        )
    }

    /// Bytes beyond the row itself.
    pub(crate) fn payload(&self) -> u64 {
        match &self.sample.value {
            Value::String(v) => v.len() as u64,
            Value::Bytes(v) => v.len() as u64,
            _ => 0,
        }
    }
}

/// The growing result of one read, charged as it grows.
pub(crate) struct Rows<'a> {
    rows: Vec<Row>,
    charge: Charge,
    filter: &'a Filter,
    cap: u64,
    /// Whether taking memory may wait. Not during a tail scan, which holds a lock
    /// the commit thread needs: a query must never stall ingest on its budget.
    pub(crate) may_wait: bool,
}

impl<'a> Rows<'a> {
    pub(crate) fn new(charge: Charge, filter: &'a Filter, cap: u64) -> Self {
        Self {
            rows: Vec::new(),
            charge,
            filter,
            cap,
            may_wait: false,
        }
    }

    /// Keeps `sample` if it passes.
    pub(crate) fn offer(&mut self, target: u32, sample: Sample) -> Result<(), Stop> {
        let passes =
            self.filter.passes_quality(sample.quality) && self.filter.passes_value(&sample.value);
        if passes {
            self.push(Row { target, sample })?;
        }
        Ok(())
    }

    fn push(&mut self, row: Row) -> Result<(), Stop> {
        if self.rows.len() == self.rows.capacity() {
            self.make_room()?;
        }
        let payload = row.payload();
        if payload > 0 {
            self.grow(payload)?;
        }
        self.rows.push(row);
        Ok(())
    }

    /// Grows by half, paying for old and new together while both exist.
    fn make_room(&mut self) -> Result<(), Stop> {
        let capacity = self.rows.capacity();
        let more = (capacity / 2).max(256);
        let old = capacity as u64 * ROW_BYTES;
        self.grow(old + more as u64 * ROW_BYTES)?;
        self.rows.reserve_exact(more);
        self.charge.shrink_to(self.charge.bytes() - old);
        Ok(())
    }

    fn grow(&mut self, bytes: u64) -> Result<(), Stop> {
        if self.charge.bytes().saturating_add(bytes) > self.cap {
            return Err(Stop::Narrow);
        }
        let taken = if self.may_wait {
            self.charge.grow(bytes)
        } else {
            self.charge.try_grow(bytes)
        };
        taken.map_err(Stop::Memory)
    }

    /// Clips `chunk` to `from..=to`: chunks are not clipped by the index.
    pub(crate) fn take(
        &mut self,
        target: u32,
        chunk: &Chunk,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<(), Stop> {
        let (low, high) = (from.as_nanos(), to.as_nanos());
        let qualities = chunk.qualities();
        for (i, t) in chunk.timestamps().iter().enumerate() {
            let quality = qualities.get(i).copied();
            let wanted =
                (low..=high).contains(t) && quality.is_some_and(|q| self.filter.passes_quality(q));
            if !wanted {
                continue;
            }
            if let Some(sample) = chunk.sample(i)
                && self.filter.passes_value(&sample.value)
            {
                self.push(Row { target, sample })?;
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> (Vec<Row>, Charge) {
        self.rows.sort_by_key(Row::key);
        self.rows.dedup_by_key(|r| r.key());
        (self.rows, self.charge)
    }
}
