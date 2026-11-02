//! Per-signal column buffers, sealed together as a generation.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;

use norn_core::{Sample, SignalId, Timestamp, Value};
use norn_storage::{Chunk, StorageError, StorageOptions};

use crate::budget::Reservation;

/// What a sample costs in the batch queue: `size_of::<Sample>()`, a payload apart.
pub(crate) const QUEUED_BYTES: u64 = 64;
/// What it costs once in columns: 26 bytes, plus `Vec` doubling's average slack.
pub(crate) const COLUMN_BYTES: u64 = 40;

/// A string or bytes payload's length.
pub(crate) fn payload(s: &Sample) -> u64 {
    match &s.value {
        Value::String(v) => v.len() as u64,
        Value::Bytes(v) => v.len() as u64,
        _ => 0,
    }
}

/// Calls `visit` for each sample of `chunk` in `from..=to`, until it stops.
pub(crate) fn scan_chunk(
    chunk: &Chunk,
    from: Timestamp,
    to: Timestamp,
    visit: &mut dyn FnMut(Sample) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let (low, high) = (from.as_nanos(), to.as_nanos());
    for (i, t) in chunk.timestamps().iter().enumerate() {
        if (low..=high).contains(t)
            && let Some(sample) = chunk.sample(i)
        {
            visit(sample)?;
        }
    }
    ControlFlow::Continue(())
}

/// The latest timestamp of `chunk` at or before `at`.
pub(crate) fn newest(chunk: &Chunk, at: Timestamp) -> Option<Timestamp> {
    let times = chunk.timestamps().iter().filter(|t| **t <= at.as_nanos());
    times.max().map(|t| Timestamp::from_nanos(*t))
}

/// One seal's samples, on their way to a segment, readable all the while.
///
/// Immutable, and shared between the store thread and queries, so a reader needs
/// no lock to scan it. Invariant: `chunks` sorts by signal.
#[derive(Debug)]
pub(crate) struct Frozen {
    pub(crate) id: u64,
    pub(crate) chunks: Vec<Chunk>,
    /// Held until no one can read the chunks from here any more.
    pub(crate) memory: Option<Reservation>,
}

impl Frozen {
    /// `signal`'s chunks. O(log n).
    pub(crate) fn of(&self, signal: SignalId) -> impl Iterator<Item = &Chunk> {
        let from = self.chunks.partition_point(|c| c.signal() < signal);
        let rest = self.chunks.iter().skip(from);
        rest.take_while(move |c| c.signal() == signal)
    }
}

/// Everything one seal handed to the store thread.
#[derive(Debug)]
pub(crate) struct Generation {
    pub(crate) frozen: Arc<Frozen>,
    /// The journal segments holding exactly these samples.
    pub(crate) journal: Vec<PathBuf>,
}

/// The chunks being filled. Memory is what the reservation inside accounts for.
/// Concurrency: owned by one thread.
#[derive(Debug)]
pub(crate) struct Buffers {
    open: HashMap<SignalId, (Chunk, u64)>,
    full: Vec<Chunk>,
    bytes: u64,
    first_at: Option<Timestamp>,
    memory: Option<Reservation>,
    max_samples: usize,
    max_payload: u64,
    next_id: u64,
}

impl Buffers {
    pub(crate) fn new(opts: &StorageOptions) -> Self {
        Self {
            open: HashMap::new(),
            full: Vec::new(),
            bytes: 0,
            first_at: None,
            memory: None,
            max_samples: opts.max_chunk_samples,
            max_payload: u64::from(opts.max_chunk_bytes) / 2,
            next_id: 0,
        }
    }

    /// Adds `s` to its signal's chunk, setting a filled chunk aside. Copies.
    pub(crate) fn push(&mut self, s: &Sample) -> Result<(), StorageError> {
        let (chunk, stored) = match self.open.entry(s.signal) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert((Chunk::new(s.signal, s.value.kind())?, 0))
            }
        };
        chunk.push(s)?;
        let size = payload(s);
        *stored += size;
        let full = chunk.len() >= self.max_samples || *stored >= self.max_payload;
        self.bytes += COLUMN_BYTES + size;
        if full && let Some((chunk, _)) = self.open.remove(&s.signal) {
            self.full.push(chunk);
        }
        Ok(())
    }

    /// Notes when this data arrived and takes over its memory account.
    pub(crate) fn adopt(&mut self, memory: Reservation, now: Timestamp) {
        self.first_at.get_or_insert(now);
        match &mut self.memory {
            Some(held) => held.merge(memory),
            None => self.memory = Some(memory),
        }
    }

    /// Bytes accounted to the open buffers.
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Whether anything is buffered.
    pub(crate) fn is_empty(&self) -> bool {
        self.open.is_empty() && self.full.is_empty()
    }

    /// Nanoseconds since the oldest buffered data arrived.
    pub(crate) fn age(&self, now: Timestamp) -> i64 {
        self.first_at
            .map_or(0, |t| now.nanos_since(t).unwrap_or(i64::MAX))
    }

    /// Empties the buffers into a generation owning `journal`.
    pub(crate) fn seal(&mut self, journal: Vec<PathBuf>) -> Generation {
        let mut chunks = std::mem::take(&mut self.full);
        chunks.extend(self.open.drain().map(|(_, (c, _))| c));
        chunks.sort_by_key(Chunk::signal);
        self.bytes = 0;
        self.first_at = None;
        self.next_id += 1;
        let frozen = Frozen {
            id: self.next_id,
            chunks,
            memory: self.memory.take(),
        };
        Generation {
            frozen: Arc::new(frozen),
            journal,
        }
    }

    /// `signal`'s chunks being filled.
    pub(crate) fn of(&self, signal: SignalId) -> impl Iterator<Item = &Chunk> {
        let open = self.open.get(&signal).map(|(c, _)| c);
        let full = self.full.iter().filter(move |c| c.signal() == signal);
        open.into_iter().chain(full)
    }
}
