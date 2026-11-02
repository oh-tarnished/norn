//! One budget per subsystem, from `Config::memory()`.

use std::sync::Arc;

use norn_core::MemoryBudgets;

use super::Budget;
use crate::clock::Clock;

/// A subsystem with its own memory budget (one per field of
/// [`MemoryBudgets`] besides the total).
///
/// `#[non_exhaustive]`: a new subsystem's budget is a minor addition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Subsystem {
    /// Samples received but not yet encoded and persisted.
    Ingest,
    /// Decoded chunks kept for reuse.
    ChunkCache,
    /// The in-memory part of the temporal index.
    Index,
    /// Working memory of in-flight queries.
    Query,
    /// Read-ahead buffers of replay sessions.
    Replay,
    /// Connection buffers and frames on the IPC transports.
    Ipc,
}

impl Subsystem {
    /// Every subsystem, in a stable order.
    pub const ALL: [Self; 6] = [
        Self::Ingest,
        Self::ChunkCache,
        Self::Index,
        Self::Query,
        Self::Replay,
        Self::Ipc,
    ];
}

/// The runtime's budgets, one per [`Subsystem`].
///
/// The subsystem budgets sum to at most `MemoryBudgets::total_bytes` (the
/// core's `ConfigBuilder` guarantees it), so honouring each one keeps the
/// process within the total without a second, shared account.
///
/// Concurrency: `Clone + Send + Sync`; clones share the same accounts.
#[derive(Debug, Clone)]
pub struct Budgets {
    ingest: Budget,
    chunk_cache: Budget,
    index: Budget,
    query: Budget,
    replay: Budget,
    ipc: Budget,
}

impl Budgets {
    /// One budget per subsystem of `memory`, all timing waits on `clock`.
    #[must_use]
    pub fn new(memory: MemoryBudgets, clock: &Arc<dyn Clock>) -> Self {
        let budget = |name, bytes| Budget::new(name, bytes, Arc::clone(clock));
        Self {
            ingest: budget("ingest_bytes", memory.ingest_bytes),
            chunk_cache: budget("chunk_cache_bytes", memory.chunk_cache_bytes),
            index: budget("index_bytes", memory.index_bytes),
            query: budget("query_bytes", memory.query_bytes),
            replay: budget("replay_bytes", memory.replay_bytes),
            ipc: budget("ipc_bytes", memory.ipc_bytes),
        }
    }

    /// The budget of `subsystem`.
    #[must_use]
    pub fn get(&self, subsystem: Subsystem) -> &Budget {
        match subsystem {
            Subsystem::Ingest => &self.ingest,
            Subsystem::ChunkCache => &self.chunk_cache,
            Subsystem::Index => &self.index,
            Subsystem::Query => &self.query,
            Subsystem::Replay => &self.replay,
            Subsystem::Ipc => &self.ipc,
        }
    }

    /// Closes every budget; see [`Budget::close`]. Idempotent.
    pub fn close(&self) {
        for s in Subsystem::ALL {
            self.get(s).close();
        }
    }
}
