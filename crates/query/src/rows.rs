//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The rows of one read, charged as they grow.

use norn_core::{Sample, Timestamp, Value};
use norn_index::ChunkLocator;
use norn_storage::Chunk;

use crate::filter::Filter;
use crate::memory::Charge;
use crate::read::Stop;

/// What one row costs held.
pub(crate) const ROW_BYTES: u64 = size_of::<Row>() as u64;

/// Where a row was written: its segment's sequence, its chunk's offset, and its
/// place in the chunk. A buffered row, not yet written, comes after every stored
/// one, in the order the tail gave it.
type Written = (u64, u64, u32);

/// An inclusive time range.
type Span = (Timestamp, Timestamp);

/// One sample and the index of its signal among the query's sorted targets.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub(crate) target: u32,
    pub(crate) sample: Sample,
    written: Written,
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
    /// The inclusive time range a stored chunk is clipped to.
    span: Span,
    rows: Vec<Row>,
    /// Buffered rows offered so far, which orders them after every stored one.
    buffered: u32,
    charge: Charge,
    filter: &'a Filter,
    cap: u64,
    /// Whether taking memory may wait. Not during a tail scan, which holds a lock
    /// the commit thread needs: a query must never stall ingest on its budget.
    pub(crate) may_wait: bool,
}

impl<'a> Rows<'a> {
    /// Rows of `span`, an inclusive time range, charged to `charge` up to `cap`.
    pub(crate) fn new(charge: Charge, filter: &'a Filter, cap: u64, span: Span) -> Self {
        Self {
            span,
            rows: Vec::new(),
            buffered: 0,
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
            let written = (u64::MAX, u64::MAX, self.buffered);
            self.buffered = self.buffered.saturating_add(1);
            self.push(Row {
                target,
                sample,
                written,
            })?;
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

    /// Clips `chunk`, read from `at`, to the rows' span: chunks are not clipped by
    /// the index.
    pub(crate) fn take(
        &mut self,
        target: u32,
        chunk: &Chunk,
        at: &ChunkLocator,
    ) -> Result<(), Stop> {
        let (low, high) = (self.span.0.as_nanos(), self.span.1.as_nanos());
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
                let place = u32::try_from(i).unwrap_or(u32::MAX);
                let written = (at.segment.seq, at.chunk.offset, place);
                self.push(Row {
                    target,
                    sample,
                    written,
                })?;
            }
        }
        Ok(())
    }

    /// The rows in key order, each (signal, time, sequence) once. Of equal keys, the
    /// first written is kept, so an answer is the same before and after compaction,
    /// which keeps write order (docs/reference/storage.md "Duplicates").
    pub(crate) fn finish(mut self) -> (Vec<Row>, Charge) {
        self.rows.sort_by_key(|r| (r.key(), r.written));
        self.rows.dedup_by_key(|r| r.key());
        (self.rows, self.charge)
    }
}

#[cfg(test)]
mod tests;
