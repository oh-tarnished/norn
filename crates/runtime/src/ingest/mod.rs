//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The write path (SPEC §16–§20); docs/reference/storage.md, "Write path", is the design.
//!
//! [`Ingest::admit`] is the only way in. Samples go admission → bounded queue →
//! commit thread (journal, then per-signal buffers) → store thread (encode in
//! parallel, write in order). Each active stream has one of each thread.

mod admit;
mod buffer;
mod clean;
mod commit;
mod commit_fail;
mod decision;
mod dedup;
mod indexing;
mod journal;
mod lifecycle;
mod record;
mod recover;
mod report;
mod scan;
mod seal;
mod shared;
mod start;
mod stats;
mod store;
mod tail;
mod upkeep;

pub use admit::THROTTLE_DELAY;
pub use decision::{AckFailure, AckLevel, Admission, Batch, Completion, Decision, Outcome, Reason};
pub use indexing::RecoverError;
pub use journal::{JOURNAL_DIR, SyncGate};
pub use report::{IndexSummary, IngestRecovery};
pub use stats::StreamStats;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use norn_core::{Config, StreamId};
use norn_index::{IndexOptions, TemporalIndex};
use norn_metadata::Registry;
use norn_storage::StorageOptions;

use self::dedup::Dedup;
use self::start::Handle;
use crate::budget::Budget;
use crate::clock::Clock;
use crate::live::Live;

/// Batches a stream queues for its commit thread.
pub(crate) const QUEUE_BATCHES: usize = 1024;

/// What the threads and admission read and never change.
#[derive(Debug)]
pub(crate) struct Context {
    pub(crate) root: PathBuf,
    pub(crate) opts: StorageOptions,
    pub(crate) registry: Arc<Registry>,
    pub(crate) budget: Budget,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) gate: Option<Arc<dyn SyncGate>>,
    /// Opened by `recover`, over the recovered segments.
    pub(crate) index: OnceLock<Arc<TemporalIndex>>,
    pub(crate) index_options: IndexOptions,
    pub(crate) durable: bool,
    pub(crate) group_interval: Duration,
    pub(crate) max_chunk_age: Duration,
    /// How long a segment stays open before maintenance asks for it sealed.
    pub(crate) max_open: Duration,
    /// The most bytes one stream may hold: three quarters of the budget, so one
    /// stream cannot starve the rest.
    pub(crate) cap: u64,
    /// Told of every committed sample; costs nothing with no subscriber.
    pub(crate) live: Arc<Live>,
}

/// The write path.
///
/// Lifecycle: [`Ingest::new`] touches nothing and spawns nothing.
/// [`recover`](Ingest::recover) must run once, before any [`admit`](Ingest::admit).
/// A stream's threads start at its first batch. [`begin_drain`](Ingest::begin_drain)
/// refuses new batches; [`close`](Ingest::close) commits what was admitted, joins
/// every thread, and is idempotent.
///
/// Memory: every byte held for a batch is reserved from the ingest budget first
/// (docs/reference/storage.md). Under a slow disk the queue fills, then admission throttles and
/// rejects; nothing grows. Concurrency: `Send + Sync`; many tasks may admit at
/// once, serialised per stream. Failure: a storage error fails that stream alone,
/// reports it on every waiting batch, and refuses further batches until restart.
#[derive(Debug)]
pub struct Ingest {
    ctx: Arc<Context>,
    streams: Mutex<HashMap<StreamId, Arc<Handle>>>,
    recovered: Mutex<HashMap<StreamId, (Dedup, StreamStats)>>,
    stats: Mutex<HashMap<StreamId, StreamStats>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    draining: AtomicBool,
    /// Cancelled with `draining`, for calls that wait on a client.
    drained: tokio_util::sync::CancellationToken,
    upkeep: upkeep::Upkeep,
}

impl Ingest {
    /// A write path for the database at `root`, drawing on `budget` (the ingest
    /// budget). Reads no file.
    #[must_use]
    pub fn new(
        root: PathBuf,
        config: &Config,
        registry: Arc<Registry>,
        budget: Budget,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let storage = config.storage();
        let cap = budget.capacity() / 4 * 3;
        let ctx = Context {
            root,
            opts: StorageOptions::from_config(config),
            registry,
            budget,
            clock,
            gate: None,
            index: OnceLock::new(),
            index_options: IndexOptions::from_config(config),
            durable: storage.durable_acks,
            group_interval: storage.group_commit_interval,
            max_chunk_age: storage.max_chunk_age,
            max_open: storage.max_segment_open_time,
            cap,
            live: Arc::new(Live::new(config.limits().max_subscriptions)),
        };
        Self {
            ctx: Arc::new(ctx),
            streams: Mutex::new(HashMap::new()),
            recovered: Mutex::new(HashMap::new()),
            stats: Mutex::new(HashMap::new()),
            threads: Mutex::new(Vec::new()),
            draining: AtomicBool::new(false),
            drained: tokio_util::sync::CancellationToken::new(),
            upkeep: upkeep::Upkeep::default(),
        }
    }

    /// Installs `gate` around every journal fsync. Before any batch only: it
    /// has no effect once a stream has started.
    #[must_use]
    pub fn sync_gate(mut self, gate: Arc<dyn SyncGate>) -> Self {
        if let Some(ctx) = Arc::get_mut(&mut self.ctx) {
            ctx.gate = Some(gate);
        }
        self
    }

    /// The temporal index, once [`recover`](Ingest::recover) has opened it. It
    /// answers for stored chunks; samples still in buffers are not in it, and
    /// reach a query through this type's [`norn_query::TailSource`].
    #[must_use]
    pub fn index(&self) -> Option<&Arc<TemporalIndex>> {
        self.ctx.index.get()
    }

    /// A stream's shared state, if its threads have started.
    fn running(&self, stream: StreamId) -> Option<Arc<shared::Shared>> {
        let streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        streams.get(&stream).map(|h| Arc::clone(&h.shared))
    }

    /// A stream's totals, as of now, buffered samples included.
    #[must_use]
    pub fn stats(&self, stream: StreamId) -> StreamStats {
        match self.running(stream) {
            Some(s) => *s.stats.lock().unwrap_or_else(PoisonError::into_inner),
            None => {
                let stats = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
                stats.get(&stream).copied().unwrap_or_default()
            }
        }
    }
}
