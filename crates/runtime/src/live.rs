//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Live subscriptions: committed samples fanned out, and never waited for.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use norn_core::{Sample, StreamId, Value};
use tokio::sync::mpsc;

use crate::budget::{Budget, Reservation};
use crate::subscription::{End, EndSlot, Subscription, leave};

/// Batches queued per subscriber before it counts as too slow. Picked: at one
/// batch per group commit that is several seconds of slack at the sustained rate.
pub const LIVE_QUEUE_BATCHES: usize = 64;

/// One committed batch, shared by every subscriber and charged once.
///
/// Concurrency: read-only in an `Arc`. Memory: its reservation, on the replay
/// budget, lasts until the slowest subscriber drops it.
#[derive(Debug)]
pub struct LiveBatch {
    /// The stream it was committed to.
    pub stream: StreamId,
    /// Its samples, in commit order, exactly as ingested.
    pub samples: Vec<Sample>,
    _held: Option<Reservation>,
}

#[derive(Debug)]
struct Subscriber {
    tx: mpsc::Sender<Arc<LiveBatch>>,
    end: EndSlot,
}

/// The fan-out behind `SubscribeEvents` in `LIVE` mode.
///
/// **It never slows ingestion.** [`publish`](Live::publish) runs on the commit
/// thread and takes no lock a subscriber holds while it waits: it hands each one a
/// reference to a shared batch with `try_send`, and does no filtering, which happens
/// on the subscriber's own task. With no subscriber it is one atomic load. A full
/// queue ends that subscriber with [`End::TooSlow`]; it is never served a gap.
///
/// Memory: a batch is charged once, to the replay budget. If the budget refuses,
/// every subscriber with a backlog is what is holding it, so each of them is ended,
/// and the batch reaches the rest uncharged, which costs one batch.
///
/// Concurrency: `Send + Sync`; many subscribe while one thread per stream publishes.
/// Failure: none of its own. Lifecycle: [`close`](Live::close) ends every subscriber
/// and refuses new ones; it is idempotent.
#[derive(Debug)]
pub struct Live {
    subscribers: RwLock<Vec<Subscriber>>,
    count: AtomicUsize,
    closed: AtomicBool,
    budget: OnceLock<Budget>,
    limit: usize,
}

impl Live {
    /// No subscribers yet, at most `limit` at once.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            subscribers: RwLock::new(Vec::new()),
            count: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            budget: OnceLock::new(),
            limit,
        }
    }

    /// Charges batches to `budget` from now on. The first call wins.
    pub fn charge_to(&self, budget: Budget) {
        let _ = self.budget.set(budget);
    }

    /// Starts following every batch committed from now on, or `None` when `limit`
    /// subscribers exist or the daemon is closing.
    #[must_use]
    pub fn subscribe(&self) -> Option<Subscription<Arc<LiveBatch>>> {
        let mut list = self.write();
        list.retain(|s| !s.tx.is_closed());
        if list.len() >= self.limit || self.closed.load(Ordering::Acquire) {
            return None;
        }
        let (tx, rx) = mpsc::channel(LIVE_QUEUE_BATCHES);
        let end = EndSlot::default();
        let keep = Arc::clone(&end);
        list.push(Subscriber { tx, end: keep });
        self.count.store(list.len(), Ordering::Release);
        Some(Subscription {
            rx,
            end,
            _guard: None,
        })
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Vec<Subscriber>> {
        self.subscribers
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Offers `samples` to every subscriber. Never waits.
    pub fn publish(&self, stream: StreamId, samples: Vec<Sample>) {
        if self.count.load(Ordering::Acquire) == 0 {
            return;
        }
        let bytes = samples.iter().map(cost).sum();
        let held = self.budget.get().and_then(|b| b.try_reserve(bytes).ok());
        let refused = held.is_none() && self.budget.get().is_some();
        let batch = Arc::new(LiveBatch {
            stream,
            samples,
            _held: held,
        });
        let mut dropped = false;
        {
            let list = self
                .subscribers
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            for s in list.iter() {
                let idle = s.tx.capacity() == s.tx.max_capacity();
                let sent = (!refused || idle) && s.tx.try_send(Arc::clone(&batch)).is_ok();
                if !sent {
                    leave(&s.end, End::TooSlow);
                    dropped = true;
                }
            }
        }
        if dropped {
            let ended = |s: &Subscriber| {
                let end = s.end.lock().unwrap_or_else(PoisonError::into_inner);
                end.is_some() || s.tx.is_closed()
            };
            let mut list = self.write();
            list.retain(|s| !ended(s));
            self.count.store(list.len(), Ordering::Release);
        }
    }

    /// Ends every subscriber and refuses new ones. Idempotent.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let mut list = self.write();
        for s in list.iter() {
            leave(&s.end, End::ShuttingDown);
        }
        list.clear();
        self.count.store(0, Ordering::Release);
    }
}

fn cost(s: &Sample) -> u64 {
    let payload = match &s.value {
        Value::String(v) => v.len(),
        Value::Bytes(v) => v.len(),
        _ => 0,
    };
    (size_of::<Sample>() + payload) as u64
}
