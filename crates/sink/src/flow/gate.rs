//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Readings waiting for their own signal, and only theirs.
//!
//! A sample of a signal Norn has not yet created would be refused, so it
//! waits; but it waits for that one signal, not for everything declared. A
//! cold start of a large deployment declares tens of thousands of signals while
//! readings pour in, and readings of the signals already made go out
//! meanwhile. When a signal is made, or given up, its waiting samples join
//! the queue behind those already there, in their own order.

use super::Flow;
use norn_protocol::common::Sample;

impl Flow {
    /// `signal` (its full name) is declared and not yet made: its samples wait.
    pub(crate) fn block(&mut self, signal: &str) {
        self.unmade.insert(signal.to_owned());
    }

    /// `signals` are made, or given up: their samples may go.
    pub(crate) fn made(&mut self, signals: &[String]) {
        for signal in signals {
            if self.unmade.remove(signal)
                && let Some(waiting) = self.blocked.remove(signal)
            {
                self.pending.extend(waiting);
            }
        }
    }

    /// Signals declared and not yet made.
    #[must_use]
    pub(crate) fn unmade(&self) -> usize {
        self.unmade.len()
    }

    /// Queues `sample`, behind its signal if that is not yet made.
    pub(super) fn route(&mut self, sample: Sample) {
        if self.unmade.contains(&sample.signal) {
            let queue = self.blocked.entry(sample.signal.clone()).or_default();
            queue.push_back(sample);
        } else {
            self.pending.push_back(sample);
        }
    }

    /// Samples waiting for their signal.
    #[must_use]
    pub(super) fn waiting(&self) -> usize {
        self.blocked
            .values()
            .map(std::collections::VecDeque::len)
            .sum()
    }

    /// The oldest sample of a waiting signal, for the byte bound once nothing
    /// ready is left to give up.
    pub(super) fn oldest_waiting(&mut self) -> Option<Sample> {
        let signal = self.blocked.keys().next()?.clone();
        let queue = self.blocked.get_mut(&signal)?;
        let oldest = queue.pop_front();
        if queue.is_empty() {
            self.blocked.remove(&signal);
        }
        oldest
    }

    /// Every waiting sample, gone, as the run ends.
    pub(super) fn drain_waiting(&mut self) -> Vec<Sample> {
        self.blocked.drain().flat_map(|(_, q)| q).collect()
    }
}
