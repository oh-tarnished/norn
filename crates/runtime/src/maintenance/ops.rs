//! The bounded table of long-running operations.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use norn_core::{StreamId, Timestamp};
use tokio_util::sync::CancellationToken;

use super::compaction::CompactionTotals;
use super::retention::RetentionReport;
use super::verify::VerifyReport;

/// Operations kept, running or finished. Past it the oldest finished one goes.
pub const MAX_OPERATIONS: usize = 256;

/// What an operation was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// Compact, restricted to samples in `interval` if given.
    Compact {
        /// The half-open interval's bounds, inclusive here.
        interval: Option<(Timestamp, Timestamp)>,
    },
    /// Read and check every chunk.
    Verify,
    /// Run the stream's retention policy.
    Retention {
        /// Remove nothing; say what would go.
        dry_run: bool,
    },
}

/// How an operation ended successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// What compaction did.
    Compacted(CompactionTotals),
    /// What verification found.
    Verified(VerifyReport),
    /// What retention removed, or would have.
    Retained(RetentionReport),
}

/// How an operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// Cancelled by a client, or by shutdown. Nothing further was changed.
    Cancelled,
    /// It could not finish; the message is for people.
    Failed(String),
}

/// One operation. Shared between the table and the worker.
///
/// Concurrency: `Send + Sync`; progress is atomics, the ending one short mutex.
#[derive(Debug)]
pub struct Operation {
    /// Its id, the last part of `operations/{id}`.
    pub id: String,
    /// The stream it works on.
    pub stream: StreamId,
    /// What it does.
    pub request: Request,
    /// When it was accepted.
    pub create_time: Timestamp,
    pub(super) done: AtomicU64,
    pub(super) total: AtomicU64,
    pub(super) cancel: CancellationToken,
    ending: Mutex<Option<(Timestamp, Result<Outcome, Failure>)>>,
}

impl Operation {
    pub(super) fn new(id: String, stream: StreamId, request: Request, now: Timestamp) -> Self {
        Self {
            id,
            stream,
            request,
            create_time: now,
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            cancel: CancellationToken::new(),
            ending: Mutex::new(None),
        }
    }

    /// Units done so far, and in total: chunks for compaction, segments for
    /// verification. The total may be zero until the work starts.
    #[must_use]
    pub fn progress(&self) -> (u64, u64) {
        let order = Ordering::Relaxed;
        (self.done.load(order), self.total.load(order))
    }

    /// When and how it ended; `None` while it runs.
    #[must_use]
    pub fn ending(&self) -> Option<(Timestamp, Result<Outcome, Failure>)> {
        self.ending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Asks it to stop. Compaction stops within a chunk, and only before its
    /// commit; verification between segments; a retention pass that has started
    /// deleting finishes. Idempotent; does nothing to a finished operation.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub(super) fn finish(&self, now: Timestamp, result: Result<Outcome, Failure>) {
        let mut ending = self.ending.lock().unwrap_or_else(PoisonError::into_inner);
        *ending = Some((now, result));
    }
}

/// Operations by age, oldest first.
///
/// Memory: at most [`MAX_OPERATIONS`] entries, each bounded by its report.
/// Operations live in memory: a restart forgets them, and `GetOperation` then
/// fails with `NOT_FOUND`. What they did to the data is durable.
#[derive(Debug, Default)]
pub(super) struct Table {
    all: Mutex<VecDeque<Arc<Operation>>>,
}

impl Table {
    /// Adds `op`, evicting the oldest finished one if full. Refuses when all are
    /// running, which the queue's own bound should prevent.
    pub(super) fn insert(&self, op: Arc<Operation>) -> bool {
        let mut all = self.all.lock().unwrap_or_else(PoisonError::into_inner);
        if all.len() >= MAX_OPERATIONS {
            let Some(at) = all.iter().position(|o| o.ending().is_some()) else {
                return false;
            };
            all.remove(at);
        }
        all.push_back(op);
        true
    }

    pub(super) fn get(&self, id: &str) -> Option<Arc<Operation>> {
        let all = self.all.lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().find(|o| o.id == id).map(Arc::clone)
    }

    /// Up to `limit` operations after id `after`, oldest first.
    pub(super) fn list(&self, after: Option<&str>, limit: usize) -> Vec<Arc<Operation>> {
        let all = self.all.lock().unwrap_or_else(PoisonError::into_inner);
        let start = after.map_or(0, |id| {
            all.iter().position(|o| o.id == id).map_or(0, |at| at + 1)
        });
        all.iter().skip(start).take(limit).map(Arc::clone).collect()
    }

    /// Forgets a finished operation; false if it is unknown or still running.
    pub(super) fn remove(&self, id: &str) -> bool {
        let mut all = self.all.lock().unwrap_or_else(PoisonError::into_inner);
        let found = all.iter().position(|o| o.id == id && o.ending().is_some());
        found.and_then(|at| all.remove(at)).is_some()
    }
}
