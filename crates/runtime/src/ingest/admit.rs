//! Admission: the synchronous decision about one batch. Never blocks on a disk.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::oneshot;

use super::buffer::{QUEUED_BYTES, payload};
use super::decision::{AckLevel, Admission, Batch, Decision, Outcome, Reason};
use super::shared::Work;
use super::start::Handle;
use super::{Ingest, QUEUE_BATCHES};
use crate::budget::BudgetError;

/// How long a throttled client waits. Picked: several group commits, and short
/// enough to be unnoticeable at the sustained rate.
pub const THROTTLE_DELAY: Duration = Duration::from_millis(100);
/// How long to wait before retrying a batch refused for lack of room.
const FULL_DELAY: Duration = Duration::from_millis(250);
/// How long to wait before retrying during shutdown: long enough to restart.
const SHUTDOWN_DELAY: Duration = Duration::from_secs(1);
/// Batches queued past which acknowledgement beyond `BUFFERED` visibly lags.
const BEHIND_BATCHES: usize = 16;

fn stored_unavailable() -> Admission {
    Admission::rejected(Decision::reject(
        Reason::StorageUnavailable,
        None,
        "this stream's storage failed; see the daemon's log, then restart it".to_owned(),
    ))
}

impl Ingest {
    /// Decides about `batch`, whole: admitted, or refused with no sample stored.
    ///
    /// Never waits on a disk: it takes the stream's admission lock, reserves, and
    /// enqueues, so it is safe on an async thread. A stream's batches are ordered
    /// by when they pass here. Starts the stream's threads the first time.
    #[must_use]
    pub fn admit(&self, batch: Batch) -> Admission {
        use std::sync::atomic::Ordering::Acquire;
        if self.draining.load(Acquire) {
            let message = "the daemon is shutting down".to_owned();
            let retry = Some(SHUTDOWN_DELAY);
            let decision = Decision::reject(Reason::ShuttingDown, retry, message);
            return Admission::rejected(decision);
        }
        if batch.level == AckLevel::Durable && !self.ctx.durable {
            let message = "this daemon is configured without fsync".to_owned();
            let decision = Decision::reject(Reason::AckLevelUnavailable, None, message);
            return Admission::rejected(decision);
        }
        match self.handle(batch.stream) {
            Some(handle) => self.admit_to(&handle, batch),
            None => stored_unavailable(),
        }
    }

    fn admit_to(&self, handle: &Handle, batch: Batch) -> Admission {
        let shared = &handle.shared;
        let mut gate = handle.lock();
        if shared.failed.load(Ordering::Acquire) {
            return stored_unavailable();
        }
        if shared.index_behind.load(Ordering::Acquire) {
            let message = "the temporal index is over its budget".to_owned();
            let retry = Some(FULL_DELAY);
            let decision = Decision::reject(Reason::MemoryExhausted, retry, message);
            return Admission::rejected(decision);
        }
        let now = self.ctx.clock.now().as_nanos();
        let until = shared.throttled_until.load(Ordering::Acquire);
        if now < until {
            let left = Duration::from_nanos(u64::try_from(until - now).unwrap_or(0));
            let message = "sent during a throttle delay".to_owned();
            let decision = Decision::reject(Reason::Throttled, Some(left), message);
            return Admission::rejected(decision);
        }
        let cost: u64 = batch
            .samples
            .iter()
            .map(|s| QUEUED_BYTES + payload(s))
            .sum();
        let (cap, held) = (self.ctx.cap, shared.held.load(Ordering::Acquire));
        if cost > cap {
            let message = format!("{cost} bytes exceeds the stream's {cap}");
            let decision = Decision::reject(Reason::BatchTooLarge, None, message);
            return Admission::rejected(decision);
        }
        let queued = shared.queued.load(Ordering::Acquire);
        if held.saturating_add(cost) > cap || queued >= QUEUE_BATCHES {
            let message = format!("{held} bytes and {queued} batches are waiting");
            let decision = Decision::reject(Reason::BufferFull, Some(FULL_DELAY), message);
            return Admission::rejected(decision);
        }
        let mut memory = match self.ctx.budget.try_reserve(cost) {
            Ok(r) => r,
            Err(e @ BudgetError::Closed { .. }) => {
                let retry = Some(SHUTDOWN_DELAY);
                let decision = Decision::reject(Reason::ShuttingDown, retry, e.to_string());
                return Admission::rejected(decision);
            }
            Err(e) => {
                let retry = Some(FULL_DELAY);
                let decision = Decision::reject(Reason::MemoryExhausted, retry, e.to_string());
                return Admission::rejected(decision);
            }
        };
        let Batch {
            mut samples, level, ..
        } = batch;
        let before = samples.len();
        samples.retain(|s| gate.dedup.admit(s));
        let duplicates = (before - samples.len()) as u64;
        let kept: u64 = samples.iter().map(|s| QUEUED_BYTES + payload(s)).sum();
        memory.shrink_to(kept);
        let (ack, completion) = if level >= AckLevel::Persisted {
            let (tx, rx) = oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        shared.held.fetch_add(kept, Ordering::AcqRel);
        let queued = shared.queued.fetch_add(1, Ordering::AcqRel) + 1;
        let work = Work {
            samples,
            memory,
            level,
            ack,
        };
        // Cannot be full: `queued` is checked under this lock, and only this lock
        // sends. It fails only if the commit thread has gone.
        let sent = gate.tx.as_ref().is_some_and(|tx| tx.try_send(work).is_ok());
        if !sent {
            shared.failed.store(true, Ordering::Release);
            return stored_unavailable();
        }
        let held = shared.held.load(Ordering::Acquire);
        let decision = self.pressure(held, queued, now, handle);
        Admission {
            decision,
            duplicates,
            reached: Some(AckLevel::Buffered),
            completion,
        }
    }

    /// The outcome for an admitted batch, from how full the stream now is. A
    /// throttle starts its delay here.
    fn pressure(&self, held: u64, queued: usize, now: i64, handle: &Handle) -> Decision {
        let high = self.ctx.cap / 4 * 3;
        if held >= high || queued >= QUEUE_BATCHES / 4 * 3 {
            let nanos = i64::try_from(THROTTLE_DELAY.as_nanos()).unwrap_or(i64::MAX);
            let until = now.saturating_add(nanos);
            handle
                .shared
                .throttled_until
                .store(until, Ordering::Release);
            let delay = Some(THROTTLE_DELAY);
            return Decision::admit(Outcome::Throttle, Reason::BufferHighWater, delay);
        }
        if held >= self.ctx.cap / 2 || queued >= BEHIND_BATCHES {
            return Decision::admit(Outcome::Buffer, Reason::WritePathBehind, None);
        }
        Decision::admit(Outcome::Accept, Reason::CapacityAvailable, None)
    }
}
