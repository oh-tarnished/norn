//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The daemon's upkeep: compaction, retention and verification (SPEC §20, §42,
//! §43). docs/reference/storage.md, "Maintenance", is the design.
//!
//! One worker does everything, one job at a time. That is the bound on the CPU and
//! disk it takes from ingest, and why two jobs can never touch one stream at once:
//! no job can delete a file another is reading.

mod compaction;
mod months;
mod ops;
mod plan;
mod report;
mod retention;
mod support;
mod verify;
mod verify_open;
mod worker;

pub use compaction::CompactionTotals;
pub use norn_storage::CompactionPhase;
pub use ops::{Failure, MAX_OPERATIONS, Operation, Outcome, Request};
pub use report::{MaintenanceReport, Removal};
pub use retention::{RemovedSegment, RetentionReport};
pub use verify::{DamagedRegion, VerifyReport};

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use norn_core::{MaintenanceConfig, StreamId, Timestamp};
use norn_metadata::Registry;
use norn_storage::SegmentName;
use tokio::sync::mpsc;

use self::ops::Table;
use self::report::Counters;
use crate::budget::Budget;
use crate::clock::Clock;
use crate::ingest::Ingest;

/// Jobs waiting for the worker. Past it a request is refused, not queued.
pub const MAX_QUEUED: usize = 16;

/// Called at each phase of a compaction, on the worker's thread, for tests that
/// must act at an exact point. Nothing in the daemon sets one.
pub type Probe = Box<dyn Fn(CompactionPhase) + Send + Sync>;

/// Why a job was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// [`MAX_QUEUED`] jobs are waiting; try later.
    Busy,
    /// The worker has stopped: the daemon is shutting down.
    Stopped,
}

/// Compaction, retention and verification.
///
/// Lifecycle: [`new`](Self::new) spawns nothing. [`run`](Self::run) blocks until
/// cancelled and returns once the worker has stopped, so a caller may then close
/// the write path. Dropping it without running leaves everything unchanged.
///
/// Concurrency: `Send + Sync`, shared in an `Arc`. Clients submit from any task; a
/// job runs on tokio's blocking pool, never on the async threads. Ingest takes no
/// lock of this type's. What it shares with ingest is the index's update mutex,
/// for one directory read per compaction, felt by the store thread, not by an
/// acknowledgement.
///
/// Memory: one run at a time, reserved from the chunk-cache budget, which nothing
/// else draws on; at most [`MAX_QUEUED`] jobs and [`MAX_OPERATIONS`] operations.
///
/// Failure: a failed job is an operation's error and a log line, and the worker
/// goes on. A compaction that fails left its inputs live.
pub struct Maintenance {
    ingest: Arc<Ingest>,
    registry: Arc<Registry>,
    memory: Budget,
    clock: Arc<dyn Clock>,
    config: MaintenanceConfig,
    table: Table,
    counters: Counters,
    /// Runs storage refused as too large for one segment, until restart.
    refused: Mutex<HashSet<(StreamId, SegmentName)>>,
    tx: mpsc::Sender<Arc<Operation>>,
    rx: Mutex<Option<mpsc::Receiver<Arc<Operation>>>>,
    next_id: AtomicU64,
    probe: OnceLock<Probe>,
}

impl std::fmt::Debug for Maintenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Maintenance").finish_non_exhaustive()
    }
}

impl Maintenance {
    /// A worker over `ingest`, drawing on `memory`, on the schedule of `config`.
    /// Touches nothing.
    #[must_use]
    pub fn new(
        ingest: Arc<Ingest>,
        registry: Arc<Registry>,
        memory: Budget,
        clock: Arc<dyn Clock>,
        config: MaintenanceConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel(MAX_QUEUED);
        Self {
            ingest,
            registry,
            memory,
            clock,
            config,
            table: Table::default(),
            counters: Counters::default(),
            refused: Mutex::new(HashSet::new()),
            tx,
            rx: Mutex::new(Some(rx)),
            next_id: AtomicU64::new(0),
            probe: OnceLock::new(),
        }
    }

    /// Sets the test probe. Before [`run`](Self::run); a second call is ignored.
    pub fn set_probe(&self, probe: Probe) {
        let _ = self.probe.set(probe);
    }

    /// Accepts `request` on `stream` as a new operation; the worker runs jobs in
    /// the order accepted. Never blocks.
    ///
    /// # Errors
    /// [`Refused`], with no operation made.
    pub fn submit(&self, stream: StreamId, request: Request) -> Result<Arc<Operation>, Refused> {
        let now = self.clock.now();
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        // Unique across restarts, so an old name finds nothing rather than another.
        let id = format!("{:x}-{n}", now.as_nanos());
        let op = Arc::new(Operation::new(id, stream, request, now));
        let sent = self.tx.try_send(Arc::clone(&op));
        match sent {
            Ok(()) if self.table.insert(Arc::clone(&op)) => Ok(op),
            Ok(()) => {
                op.cancel();
                Err(Refused::Busy)
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(Refused::Busy),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(Refused::Stopped),
        }
    }

    /// The operation with `id`, if it is still kept.
    #[must_use]
    pub fn operation(&self, id: &str) -> Option<Arc<Operation>> {
        self.table.get(id)
    }

    /// Up to `limit` operations after id `after`, oldest first.
    #[must_use]
    pub fn operations(&self, after: Option<&str>, limit: usize) -> Vec<Arc<Operation>> {
        self.table.list(after, limit)
    }

    /// Forgets a finished operation; false if unknown or still running.
    pub fn forget(&self, id: &str) -> bool {
        self.table.remove(id)
    }

    /// What maintenance has done since start, for `GetServer`.
    #[must_use]
    pub fn report(&self) -> MaintenanceReport {
        self.counters.report()
    }

    /// The time this daemon's clock reads, for an operation's bookkeeping.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }
}
