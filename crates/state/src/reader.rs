//! `StateAt(T)`, directly from the index.

use std::sync::Arc;

use norn_core::{Sample, Timestamp, Value};
use norn_query::{Charge, Engine, QueryError, QueryStats, Refused, Result, Target};

/// What an entry costs held, apart from its names and payload.
const ENTRY_BYTES: u64 = size_of::<SignalState>() as u64;
/// Signals past which an answer is worth more than one thread. Picked from the
/// cost of one lookup, about 25 µs, and of starting a thread, about as much.
const PARALLEL_FROM: usize = 512;
/// The most threads one answer takes. Picked: past this the shared index and
/// the page cache, not the decoding, are what is left. See docs/performance.md.
const MAX_THREADS: usize = 8;

/// What one signal held at the instant.
///
/// Concurrency: plain owned data.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalState {
    /// The signal's resource name.
    pub signal: Arc<str>,
    /// The latest sample at or before the instant, ties going to the highest
    /// sequence. `None` if there is none, or if it is older than `max_age`.
    pub sample: Option<Sample>,
}

/// The state of some signals at one instant, sorted by signal name.
///
/// Holds its memory charge: dropping it returns it. Concurrency: `Send + Sync`,
/// read-only once built.
#[derive(Debug)]
pub struct State {
    /// When the state describes.
    pub at: Timestamp,
    entries: Vec<SignalState>,
    stats: QueryStats,
    _charges: Vec<Charge>,
}

impl State {
    /// One entry a signal, in signal name order.
    #[must_use]
    pub fn entries(&self) -> &[SignalState] {
        &self.entries
    }

    /// What building it read.
    #[must_use]
    pub fn stats(&self) -> QueryStats {
        self.stats
    }

    /// What it would cost the wire, roughly, for cutting pages.
    #[must_use]
    pub fn encoded_bytes(entry: &SignalState) -> u64 {
        entry_bytes(entry)
    }
}

fn entry_bytes(entry: &SignalState) -> u64 {
    let payload = match entry.sample.as_ref().map(|s| &s.value) {
        Some(Value::String(v)) => v.len() as u64,
        Some(Value::Bytes(v)) => v.len() as u64,
        _ => 0,
    };
    ENTRY_BYTES + entry.signal.len() as u64 + payload
}

/// Answers `StateAt(T)`.
///
/// Every sample is stored, so a state needs no checkpoint: each signal's latest
/// sample at or before `T` is one lookup in the index and one small read, and
/// buffered samples come from the query engine's tail. Measurements and the
/// checkpoint decision: docs/reference/storage.md, "State checkpoints".
///
/// Concurrency: `Send + Sync`, one per daemon. Each call is independent, and blocks
/// on the disk. Performance: an answer costs in proportion to the signals selected.
/// Above 512 of them it reads on up to 8 scoped threads, joined before it returns,
/// because chunks are per signal and decoding them is the cost.
#[derive(Debug, Clone)]
pub struct StateReader {
    engine: Arc<Engine>,
}

impl StateReader {
    /// A reader through `engine`, which charges what it reads to its own memory.
    #[must_use]
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }

    /// The state of `targets` at `at`.
    ///
    /// A sample at exactly `at` counts. With `max_age`, in nanoseconds, a sample
    /// older than `at - max_age` is reported as no sample: unknown, not stale. A
    /// sample exactly that old is kept.
    ///
    /// The answer describes the moment each signal was read: a sample acknowledged
    /// at `PERSISTED` before this call is seen, and one arriving during it may or
    /// may not be. Pin it ([`crate::Pins`]) so pages agree.
    ///
    /// # Errors
    /// [`norn_query::QueryError::Corrupt`] for a damaged chunk,
    /// [`norn_query::QueryError::Memory`], [`norn_query::QueryError::Raced`].
    pub fn state_at(
        &self,
        targets: Vec<Target>,
        at: Timestamp,
        max_age: Option<i64>,
    ) -> Result<State> {
        let from = max_age.map_or(Timestamp::MIN, |age| {
            Timestamp::from_nanos(at.as_nanos().saturating_sub(age))
        });
        let mut targets = targets;
        targets.sort_by(|a, b| a.name.cmp(&b.name));
        let mut charge = self.engine.charge();
        charge.grow(targets.len() as u64 * ENTRY_BYTES)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(targets.len())
            .map_err(|_| Refused::Exhausted {
                requested: targets.len() as u64 * ENTRY_BYTES,
            })?;
        let parts = self.parts(targets.len());
        let share = targets.len().div_ceil(parts.max(1)).max(1);
        let read = |list: &[Target]| self.read(list, from, at);
        let done: Vec<Result<Part>> = if parts <= 1 {
            vec![read(&targets)]
        } else {
            std::thread::scope(|scope| {
                let spawned: Vec<_> = targets
                    .chunks(share)
                    .map(|list| scope.spawn(move || read(list)))
                    .collect();
                let join = |h: std::thread::ScopedJoinHandle<'_, Result<Part>>| {
                    h.join().unwrap_or(Err(QueryError::Raced))
                };
                spawned.into_iter().map(join).collect()
            })
        };
        let mut stats = QueryStats::default();
        let mut charges = vec![charge];
        for part in done {
            let part = part?;
            entries.extend(part.entries);
            charges.push(part.charge);
            stats.chunks_read += part.stats.chunks_read;
            stats.bytes_read += part.stats.bytes_read;
            stats.races += part.stats.races;
        }
        Ok(State {
            at,
            entries,
            stats,
            _charges: charges,
        })
    }

    /// How many threads to read on.
    fn parts(&self, targets: usize) -> usize {
        if targets < PARALLEL_FROM {
            return 1;
        }
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        cores.min(MAX_THREADS)
    }

    fn read(&self, targets: &[Target], from: Timestamp, at: Timestamp) -> Result<Part> {
        let mut newest = self.engine.newest();
        let mut charge = self.engine.charge();
        let mut entries = Vec::with_capacity(targets.len());
        for target in targets {
            let target = Arc::new(target.clone());
            let sample = newest.at(&target, from, at)?;
            let entry = SignalState {
                signal: Arc::clone(&target.name),
                sample,
            };
            charge.grow(entry_bytes(&entry).saturating_sub(ENTRY_BYTES))?;
            entries.push(entry);
        }
        let stats = newest.stats();
        Ok(Part {
            entries,
            charge,
            stats,
        })
    }
}

/// What one thread read.
struct Part {
    entries: Vec<SignalState>,
    charge: Charge,
    stats: QueryStats,
}
