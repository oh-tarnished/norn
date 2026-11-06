//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Readings between the adapter and Norn, as a pure state machine: what may go
//! out now, and what an answer means. No I/O and no clock: every call is told
//! the time, so a whole outage is a table test (`tests`).
//!
//! A sample stays here until Norn has acknowledged it at the level asked for,
//! so a call dying resends exactly what it might have lost; Norn counts a
//! resend of an equal signal, time and sequence as a duplicate, and each sample
//! keeps those for life (protocol.md, "Writing a client" step 4).

use crate::counts::{Backoff, Counts};
use norn_protocol::common::Sample;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest};
use prost::Message as _;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::Duration;

mod answer;
mod gate;
mod hold;
#[cfg(test)]
mod tests;

pub(crate) use answer::Outcome;
use hold::Ledger;

/// Batches on the wire at once. Well under Norn's `max_pending_batches` of 256:
/// more than a few only queues in the daemon what could wait here.
const WINDOW: usize = 16;
/// Limits used until a daemon publishes its own: Norn's defaults.
const MAX_SAMPLES: usize = 10_000;
const MAX_BYTES: usize = 4 << 20;

/// A batch on the wire.
#[derive(Debug)]
struct Inflight {
    samples: Vec<Sample>,
    /// Its first response has come, and admitted it.
    admitted: bool,
}

/// Readings on their way to one stream. Not shared by itself: the sink keeps
/// it behind one lock.
#[derive(Debug)]
pub(crate) struct Flow {
    stream: String,
    ack: AcknowledgementLevel,
    /// Bytes held at most.
    capacity: usize,
    ledger: Ledger,
    pending: VecDeque<Sample>,
    /// Signals declared and not yet made, and their samples (`gate`).
    unmade: HashSet<String>,
    blocked: HashMap<String, VecDeque<Sample>>,
    inflight: BTreeMap<i64, Inflight>,
    next_batch: i64,
    max_samples: usize,
    max_bytes: usize,
    window: usize,
    not_before: Duration,
    backoff: Backoff,
    counts: Counts,
}

impl Flow {
    /// A flow to `stream` (its full name), each batch to reach `ack`, holding
    /// about `capacity` bytes at most (see `hold`).
    #[must_use]
    pub(crate) fn new(stream: String, ack: AcknowledgementLevel, capacity: usize) -> Self {
        Self {
            stream,
            ack,
            capacity,
            ledger: Ledger::default(),
            pending: VecDeque::new(),
            unmade: HashSet::new(),
            blocked: HashMap::new(),
            inflight: BTreeMap::new(),
            next_batch: 1,
            max_samples: MAX_SAMPLES,
            max_bytes: MAX_BYTES,
            window: WINDOW,
            not_before: Duration::ZERO,
            backoff: Backoff::default(),
            counts: Counts::default(),
        }
    }

    /// Takes the limits a daemon published. Three quarters of its byte limit,
    /// leaving room for the framing an encoded size does not count.
    pub(crate) fn limit(&mut self, samples: i32, bytes: i64, pending: i32) {
        let or = |n, default| {
            usize::try_from(n)
                .ok()
                .filter(|n| *n > 0)
                .unwrap_or(default)
        };
        self.max_samples = or(i64::from(samples), MAX_SAMPLES);
        self.max_bytes = or(bytes, MAX_BYTES) / 4 * 3;
        self.window = or(i64::from(pending), WINDOW).min(WINDOW);
    }

    /// Whether a batch may go out at `now`.
    #[must_use]
    pub(crate) fn ready(&self, now: Duration) -> bool {
        !self.pending.is_empty() && self.inflight.len() < self.window && now >= self.not_before
    }

    /// The next batch, if one may go out at `now`; it stays held until it is
    /// answered. As many samples as fit Norn's two limits, in order.
    pub(crate) fn take(&mut self, now: Duration) -> Option<AppendSamplesRequest> {
        if !self.ready(now) {
            return None;
        }
        let (mut size, mut samples) = (0, Vec::new());
        while samples.len() < self.max_samples
            && let Some(next) = self.pending.front()
        {
            size += next.encoded_len() + 4;
            if size > self.max_bytes && !samples.is_empty() {
                break;
            }
            samples.extend(self.pending.pop_front());
        }
        let batch_id = self.next_batch;
        self.next_batch += 1;
        let batch = AppendSamplesRequest {
            stream: self.stream.clone(),
            batch_id,
            samples: samples.clone(),
            ack_level: self.ack as i32,
        };
        let admitted = false;
        self.inflight
            .insert(batch_id, Inflight { samples, admitted });
        Some(batch)
    }

    /// When a batch held back only by a delay may go, if one is.
    #[must_use]
    pub(crate) fn wake(&self, now: Duration) -> Option<Duration> {
        let held_back = !self.pending.is_empty() && now < self.not_before;
        held_back.then_some(self.not_before)
    }

    /// The call died: every batch unanswered is sent again on the next, whose
    /// batch IDs start over, as Norn requires of a new call.
    pub(crate) fn lost(&mut self) {
        for batch in std::mem::take(&mut self.inflight).into_values().rev() {
            self.requeue(batch.samples);
        }
        self.next_batch = 1;
    }

    /// Gives up everything held, as the run ends: how many samples went.
    pub(crate) fn abandon(&mut self) -> usize {
        self.lost();
        let waiting = self.drain_waiting();
        let gone = self.pending.len() + waiting.len();
        for sample in self.pending.drain(..).chain(waiting) {
            self.ledger.remove(&sample);
        }
        gone
    }

    /// Samples not yet acknowledged.
    #[must_use]
    pub(crate) fn held(&self) -> usize {
        let on_wire: usize = self.inflight.values().map(|b| b.samples.len()).sum();
        self.pending.len() + self.waiting() + on_wire
    }

    /// What has been counted so far.
    #[must_use]
    pub(crate) fn counts(&self) -> Counts {
        self.counts
    }

    /// Puts `samples` back at the front, in their order.
    fn requeue(&mut self, samples: Vec<Sample>) {
        for sample in samples.into_iter().rev() {
            self.pending.push_front(sample);
        }
    }
}
