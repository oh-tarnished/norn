//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a flow may hold, in bytes, and how it is shared between signals.
//!
//! The bound is on bytes, not samples: a sample of a long text costs many
//! times one of a number, and memory is what runs out. A sample's cost is its
//! encoded size plus the struct that holds it, close to what it takes in the
//! adapter's heap.
//!
//! Within the bound, each signal may hold at most a share of it
//! ([`SHARE`]th, never under [`MIN_SHARE`]). Past its share a signal's new
//! samples are given up and counted as `gated`, so one chatty signal (a
//! burst of changes, a flapping flag) cannot fill the buffer and starve the
//! others of room. Past the whole bound the oldest samples go, whoever's.

use super::Flow;
use norn_protocol::common::Sample;
use prost::Message as _;
use std::collections::HashMap;

/// The fraction of the buffer one signal may hold: one sixty-fourth.
const SHARE: usize = 64;
/// The least share, so a small buffer still lets a signal hold a backlog.
const MIN_SHARE: usize = 64 << 10;

/// What one sample costs to hold.
#[must_use]
pub(crate) fn cost(sample: &Sample) -> usize {
    sample.encoded_len() + std::mem::size_of::<Sample>()
}

/// Bytes held, in all and by signal. Plain data.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    bytes: usize,
    by_signal: HashMap<String, usize>,
}

impl Ledger {
    /// `sample` is held now.
    pub(crate) fn add(&mut self, sample: &Sample) {
        let c = cost(sample);
        self.bytes += c;
        *self.by_signal.entry(sample.signal.clone()).or_default() += c;
    }

    /// `sample` is held no more: acknowledged or given up.
    pub(crate) fn remove(&mut self, sample: &Sample) {
        let c = cost(sample);
        self.bytes = self.bytes.saturating_sub(c);
        if let Some(n) = self.by_signal.get_mut(&sample.signal) {
            *n = n.saturating_sub(c);
            if *n == 0 {
                self.by_signal.remove(&sample.signal);
            }
        }
    }

    /// Bytes held in all.
    #[must_use]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Bytes `signal` holds.
    #[must_use]
    fn of(&self, signal: &str) -> usize {
        self.by_signal.get(signal).copied().unwrap_or(0)
    }
}

impl Flow {
    /// The most one signal may hold.
    #[must_use]
    pub(crate) fn share(&self) -> usize {
        (self.capacity / SHARE).max(MIN_SHARE)
    }

    /// Queues `samples`. A signal past its [`share`](Flow::share) has its
    /// new samples given up; past the capacity the oldest waiting are given
    /// up, whoever's. Both are counted: the bound is on memory, so it
    /// cannot be a wait. Returns how many went, both ways.
    pub(crate) fn offer(&mut self, samples: Vec<Sample>) -> usize {
        let share = self.share();
        let mut gated = 0;
        for sample in samples {
            if self.ledger.of(&sample.signal) + cost(&sample) > share {
                gated += 1;
                continue;
            }
            self.ledger.add(&sample);
            self.route(sample);
        }
        let mut gone = 0;
        while self.ledger.bytes() > self.capacity {
            let Some(oldest) = self.pending.pop_front().or_else(|| self.oldest_waiting()) else {
                break;
            };
            self.ledger.remove(&oldest);
            gone += 1;
        }
        self.counts.overflowed += gone as u64;
        self.counts.gated += gated as u64;
        gone + gated
    }

    /// Bytes held, waiting or on the wire.
    #[must_use]
    pub(crate) fn held_bytes(&self) -> usize {
        self.ledger.bytes()
    }
}
