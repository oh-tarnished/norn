//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Counters proving what a lookup touched (SPEC §34, §22).

use std::sync::atomic::{AtomicU64, Ordering};

/// What the index has done since it was opened; see
/// [`crate::TemporalIndex::stats`].
///
/// `segment_opens` counts segment *files* the index itself opened, which
/// happens only to build or repair detail. A healthy lookup leaves it alone.
///
/// Concurrency: a copy; each field was read separately, so under load they may
/// not describe one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct IndexStats {
    /// Segment files opened.
    pub segment_opens: u64,
    /// Bytes read from segment files: headers and directories.
    pub segment_bytes_read: u64,
    /// Sidecar files opened for a head or a page.
    pub detail_opens: u64,
    /// Bytes read from sidecar files.
    pub detail_bytes_read: u64,
    /// Sidecars answered from the segment's directory instead, because they
    /// were missing, damaged or stale.
    pub detail_repairs: u64,
    /// Repaired sidecars that could not be written back. Answers stay correct,
    /// and the next lookup repairs again.
    pub detail_write_failures: u64,
    /// Times `summary.nidx` was written.
    pub summary_writes: u64,
    /// Page-cache hits.
    pub cache_hits: u64,
    /// Page-cache misses.
    pub cache_misses: u64,
}

/// The live counters behind [`IndexStats`].
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) segment_opens: AtomicU64,
    pub(crate) segment_bytes_read: AtomicU64,
    pub(crate) detail_opens: AtomicU64,
    pub(crate) detail_bytes_read: AtomicU64,
    pub(crate) detail_repairs: AtomicU64,
    pub(crate) detail_write_failures: AtomicU64,
    pub(crate) summary_writes: AtomicU64,
    pub(crate) cache_hits: AtomicU64,
    pub(crate) cache_misses: AtomicU64,
}

/// Adds `n` to `counter`. Relaxed: these are statistics, ordering nothing.
pub(crate) fn add(counter: &AtomicU64, n: u64) {
    counter.fetch_add(n, Ordering::Relaxed);
}

impl Counters {
    /// A copy of every counter.
    pub(crate) fn snapshot(&self) -> IndexStats {
        let get = |c: &AtomicU64| c.load(Ordering::Relaxed);
        IndexStats {
            segment_opens: get(&self.segment_opens),
            segment_bytes_read: get(&self.segment_bytes_read),
            detail_opens: get(&self.detail_opens),
            detail_bytes_read: get(&self.detail_bytes_read),
            detail_repairs: get(&self.detail_repairs),
            detail_write_failures: get(&self.detail_write_failures),
            summary_writes: get(&self.summary_writes),
            cache_hits: get(&self.cache_hits),
            cache_misses: get(&self.cache_misses),
        }
    }
}
