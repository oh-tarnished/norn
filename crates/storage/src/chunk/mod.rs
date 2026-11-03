//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A chunk: one signal's samples over a time range, held column-wise.
//!
//! On disk a chunk is a frame (see [`crate::format::frame`]) whose body is
//! four sections, each a `u32` length and a norn-codec block sealed in its
//! compression envelope: timestamps, values, quality codes, sequences.
//! Each column is encoded and compressed independently, and the whole body
//! is checksummed, so any chunk decodes without any other (SPEC §13).

mod column;
pub(crate) mod decode;
pub(crate) mod encode;

pub use column::Column;
pub use encode::EncodedChunk;

use norn_core::{Quality, Sample, Sequence, SignalId, Timestamp, Value, ValueKind};

use crate::error::{Result, StorageError};

/// One signal's samples, column-wise, in the order they were appended.
///
/// Invariant: the four columns always have equal length, and every value is
/// of the column's kind; [`Chunk::push`] and [`Chunk::extend`] keep it.
/// Samples are kept in append order, not sorted: storage preserves exactly
/// what was written, and the timestamp codec handles any order.
///
/// Memory: about 18 bytes per sample plus the value at its native width.
/// Concurrency: owned plain data; `Send + Sync`.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    signal: SignalId,
    timestamps: Vec<i64>,
    values: Column,
    qualities: Vec<Quality>,
    sequences: Vec<u64>,
}

impl Chunk {
    /// An empty chunk for `signal`, holding values of `kind`.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for a kind this build cannot store.
    pub fn new(signal: SignalId, kind: ValueKind) -> Result<Self> {
        Ok(Self {
            signal,
            timestamps: Vec::new(),
            values: Column::new(kind)?,
            qualities: Vec::new(),
            sequences: Vec::new(),
        })
    }

    /// A chunk of `samples`, all of one signal and one value kind.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for no samples, mixed signals or
    /// mixed value kinds.
    pub fn from_samples(samples: &[Sample]) -> Result<Self> {
        let first = samples
            .first()
            .ok_or(StorageError::InvalidChunk("no samples"))?;
        let mut chunk = Self::new(first.signal, first.value.kind())?;
        for s in samples {
            chunk.push(s)?;
        }
        Ok(chunk)
    }

    /// Appends one sample.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for another signal or value kind; the
    /// chunk is unchanged.
    pub fn push(&mut self, s: &Sample) -> Result<()> {
        if s.signal != self.signal {
            return Err(StorageError::InvalidChunk("sample of another signal"));
        }
        self.values.push(&s.value)?;
        self.timestamps.push(s.timestamp.as_nanos());
        self.qualities.push(s.quality);
        self.sequences.push(s.sequence.get());
        Ok(())
    }

    /// Appends every sample of `other`, keeping their order.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for another signal or value kind.
    pub fn extend(&mut self, other: &Self) -> Result<()> {
        if other.signal != self.signal {
            return Err(StorageError::InvalidChunk("chunk of another signal"));
        }
        self.values.extend(&other.values)?;
        self.timestamps.extend_from_slice(&other.timestamps);
        self.qualities.extend_from_slice(&other.qualities);
        self.sequences.extend_from_slice(&other.sequences);
        Ok(())
    }

    /// The `i`th sample, copied out, or `None` past the end.
    #[must_use]
    pub fn sample(&self, i: usize) -> Option<Sample> {
        let ts = Timestamp::from_nanos(*self.timestamps.get(i)?);
        let value: Value = self.values.get(i)?;
        let quality = *self.qualities.get(i)?;
        let seq = Sequence::new(*self.sequences.get(i)?);
        Some(Sample::new(ts, self.signal, value, quality, seq))
    }

    /// Every sample, copied out, in order.
    pub fn samples(&self) -> impl Iterator<Item = Sample> + '_ {
        (0..self.len()).map_while(|i| self.sample(i))
    }

    /// The signal.
    #[must_use]
    pub fn signal(&self) -> SignalId {
        self.signal
    }

    /// The value kind.
    #[must_use]
    pub fn kind(&self) -> ValueKind {
        self.values.kind()
    }

    /// Number of samples.
    #[must_use]
    pub fn len(&self) -> usize {
        self.timestamps.len()
    }

    /// Whether the chunk holds no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Timestamps in nanoseconds, in append order.
    #[must_use]
    pub fn timestamps(&self) -> &[i64] {
        &self.timestamps
    }

    /// The value column.
    #[must_use]
    pub fn values(&self) -> &Column {
        &self.values
    }

    /// Qualities, in append order.
    #[must_use]
    pub fn qualities(&self) -> &[Quality] {
        &self.qualities
    }

    /// Raw sequence numbers, in append order.
    #[must_use]
    pub fn sequences(&self) -> &[u64] {
        &self.sequences
    }

    /// The earliest and latest timestamp, or `None` when empty.
    #[must_use]
    pub fn time_range(&self) -> Option<(Timestamp, Timestamp)> {
        let min = self.timestamps.iter().min()?;
        let max = self.timestamps.iter().max()?;
        Some((Timestamp::from_nanos(*min), Timestamp::from_nanos(*max)))
    }

    /// Builds a chunk from decoded columns; the decoder checks lengths.
    pub(crate) fn from_parts(
        signal: SignalId,
        timestamps: Vec<i64>,
        values: Column,
        qualities: Vec<Quality>,
        sequences: Vec<u64>,
    ) -> Self {
        Self {
            signal,
            timestamps,
            values,
            qualities,
            sequences,
        }
    }
}
