//! The samples a query must see that the index does not hold yet.

use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use norn_core::Config;
use norn_core::{Sample, SignalId, StreamId, Timestamp};
use norn_query::{Engine, QueryLimits, QueryMemory, TailSource};
use norn_storage::{SegmentName, StorageOptions};

use super::Ingest;
use super::buffer::{Buffers, Frozen, Generation, newest, scan_chunk};
use crate::budget::Budget;

/// What the lock guards.
#[derive(Debug)]
pub(crate) struct TailState {
    pub(crate) buffers: Buffers,
    frozen: VecDeque<Arc<Frozen>>,
}

/// One stream's samples from buffering until the index has them.
///
/// **The contract that makes queries exact.** A sample enters here before its
/// `PERSISTED` acknowledgement, moves from `buffers` to `frozen` under one write
/// lock, so a reader never finds it in neither, and leaves only after
/// `publish_chunks` or `segment_sealed` has put it in the index (`store::run`).
/// [`norn_query::TailSource`] builds exactly-once on that.
///
/// While the index is behind, a generation stays here, its memory reserved, which
/// is also why a stream whose index is behind is refused new batches.
///
/// Memory: reserved from the ingest budget, by admission, and held until the
/// generation leaves here and no reader holds it. Concurrency: the commit thread
/// writes, for one batch or one seal at a time. A reader holds the read lock for
/// one signal's open chunk at most, about 2 ms at 86,400 samples, and a commit
/// waits that long. Frozen generations are scanned with no lock.
#[derive(Debug)]
pub(crate) struct Tail {
    state: RwLock<TailState>,
}

impl Tail {
    pub(crate) fn new(buffers: Buffers) -> Self {
        let frozen = VecDeque::new();
        Self {
            state: RwLock::new(TailState { buffers, frozen }),
        }
    }

    pub(crate) fn read(&self) -> RwLockReadGuard<'_, TailState> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, TailState> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Seals the buffers: every sample moves, in one step, to `frozen`.
    pub(crate) fn seal(&self, journal: Vec<std::path::PathBuf>) -> Generation {
        let mut state = self.write();
        let generation = state.buffers.seal(journal);
        state.frozen.push_back(Arc::clone(&generation.frozen));
        generation
    }

    /// Forgets generations up to `id`: the index has every one of their chunks.
    pub(crate) fn retire(&self, id: u64) {
        self.write().frozen.retain(|f| f.id > id);
    }

    fn scan(
        &self,
        signal: SignalId,
        from: Timestamp,
        to: Timestamp,
        visit: &mut dyn FnMut(Sample) -> ControlFlow<()>,
    ) {
        let frozen = {
            let state = self.read();
            for chunk in state.buffers.of(signal) {
                if scan_chunk(chunk, from, to, visit).is_break() {
                    return;
                }
            }
            state.frozen.clone()
        };
        for generation in &frozen {
            for chunk in generation.of(signal) {
                if scan_chunk(chunk, from, to, visit).is_break() {
                    return;
                }
            }
        }
    }

    fn newest(&self, signal: SignalId, at: Timestamp) -> Option<Timestamp> {
        let state = self.read();
        let open = state.buffers.of(signal).filter_map(|c| newest(c, at)).max();
        let frozen = state.frozen.iter().flat_map(|g| g.of(signal));
        open.max(frozen.filter_map(|c| newest(c, at)).max())
    }
}

impl Ingest {
    /// Where live subscribers attach.
    #[must_use]
    pub fn live(&self) -> &Arc<crate::live::Live> {
        &self.ctx.live
    }

    /// The query engine over this write path, charging `memory`, the query
    /// budget. `None` before [`recover`](Ingest::recover) has opened the index.
    #[must_use]
    pub fn query_engine(self: &Arc<Self>, memory: Budget, config: &Config) -> Option<Arc<Engine>> {
        let index = Arc::clone(self.index()?);
        let tail: Arc<dyn TailSource> = Arc::clone(self) as Arc<dyn TailSource>;
        let memory: Arc<dyn QueryMemory> = Arc::new(memory);
        let limits = QueryLimits::from_config(config);
        let storage = StorageOptions::from_config(config);
        Some(Arc::new(Engine::new(index, tail, memory, limits, storage)))
    }

    /// The engine replay reads through, charging `memory`, the replay budget.
    ///
    /// A slice may take an eighth of the budget, as a query's may of its own: replay
    /// reads in time order, where every slice decodes each chunk it overlaps again,
    /// so a narrow slice costs many decodes (docs/performance.md). A page takes at
    /// most a quarter of a fair share, `replay_bytes / max_replay_sessions`, since
    /// pages wait for subscribers. When sessions together want more, the budget
    /// refuses and the engine narrows their slices. `None` before
    /// [`recover`](Ingest::recover).
    #[must_use]
    pub fn replay_engine(self: &Arc<Self>, memory: Budget, config: &Config) -> Option<Arc<Engine>> {
        let index = Arc::clone(self.index()?);
        let tail: Arc<dyn TailSource> = Arc::clone(self) as Arc<dyn TailSource>;
        let mut limits = QueryLimits::from_config(config);
        let sessions = config.limits().max_replay_sessions.max(1) as u64;
        let share = memory.capacity() / sessions;
        limits.slice_bytes = limits.slice_bytes.min((memory.capacity() / 8).max(1 << 16));
        limits.page_bytes = limits.page_bytes.min((share / 4).max(1 << 16));
        limits.open_files = limits.open_files.min(4);
        let memory: Arc<dyn QueryMemory> = Arc::new(memory);
        let storage = StorageOptions::from_config(config);
        Some(Arc::new(Engine::new(index, tail, memory, limits, storage)))
    }
}

impl TailSource for Ingest {
    fn scan(
        &self,
        stream: StreamId,
        signal: SignalId,
        from: Timestamp,
        to: Timestamp,
        visit: &mut dyn FnMut(Sample) -> ControlFlow<()>,
    ) {
        if let Some(shared) = self.running(stream) {
            shared.tail.scan(signal, from, to, visit);
        }
    }

    fn newest_at_or_before(
        &self,
        stream: StreamId,
        signal: SignalId,
        at: Timestamp,
    ) -> Option<Timestamp> {
        self.running(stream)?.tail.newest(signal, at)
    }

    fn damaged(&self, stream: StreamId, segment: SegmentName, offset: u64, samples: u32) {
        self.note_damage(stream, segment, offset, u64::from(samples));
    }
}
