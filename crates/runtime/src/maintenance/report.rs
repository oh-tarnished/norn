//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What maintenance has done since the daemon started.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use norn_core::Timestamp;
use norn_storage::CompactionOutcome;

use super::retention::RetentionReport;

/// The removals `GetServer` lists.
pub(super) const MAX_RECENT: usize = 16;

/// One retention pass that removed something.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Removal {
    /// The stream's resource ID.
    pub stream: String,
    /// When the pass finished.
    pub at: Timestamp,
    /// Segments removed.
    pub segments: u64,
    /// Samples they held.
    pub samples: u64,
    /// Bytes they took.
    pub bytes: u64,
}

/// A copy of the counters, for `GetServer`.
///
/// Concurrency: owned plain data. Memory: at most 16 removals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MaintenanceReport {
    /// Compactions that committed.
    pub compactions: u64,
    /// Input segments they replaced.
    pub compacted_segments: u64,
    /// Bytes they saved; negative if they grew the stream.
    pub saved_bytes: i64,
    /// Compactions that failed, leaving their inputs.
    pub failures: u64,
    /// Segments retention removed.
    pub retention_segments: u64,
    /// Samples those held.
    pub retention_samples: u64,
    /// Bytes those took.
    pub retention_bytes: u64,
    /// The latest removals, newest first.
    pub recent: Vec<Removal>,
}

#[derive(Debug, Default)]
pub(super) struct Counters {
    compactions: AtomicU64,
    segments: AtomicU64,
    saved: AtomicI64,
    failures: AtomicU64,
    removed_segments: AtomicU64,
    removed_samples: AtomicU64,
    removed_bytes: AtomicU64,
    pub(super) recent: Mutex<VecDeque<Removal>>,
}

impl Counters {
    pub(super) fn compacted(&self, inputs: u64, outcome: &CompactionOutcome) {
        let saved = i128::from(outcome.bytes_before) - i128::from(outcome.output.bytes);
        let saved = i64::try_from(saved).unwrap_or(0);
        self.compactions.fetch_add(1, Ordering::Relaxed);
        self.segments.fetch_add(inputs, Ordering::Relaxed);
        self.saved.fetch_add(saved, Ordering::Relaxed);
    }

    pub(super) fn failed(&self) {
        self.failures.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn removed(&self, r: &RetentionReport) {
        self.removed_segments
            .fetch_add(r.segments, Ordering::Relaxed);
        self.removed_samples.fetch_add(r.samples, Ordering::Relaxed);
        self.removed_bytes.fetch_add(r.bytes, Ordering::Relaxed);
    }

    pub(super) fn report(&self) -> MaintenanceReport {
        let order = Ordering::Relaxed;
        let recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        MaintenanceReport {
            compactions: self.compactions.load(order),
            compacted_segments: self.segments.load(order),
            saved_bytes: self.saved.load(order),
            failures: self.failures.load(order),
            retention_segments: self.removed_segments.load(order),
            retention_samples: self.removed_samples.load(order),
            retention_bytes: self.removed_bytes.load(order),
            recent: recent.iter().cloned().collect(),
        }
    }
}
