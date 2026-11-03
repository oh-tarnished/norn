//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The bounds a query runs under, from the engine configuration.

use norn_core::Config;

/// The most encoded bytes one response aims at. Below gRPC's default 4 MiB limit
/// on what a client decodes, so a client needs no configuration to read a page.
const PAGE_BYTES: u64 = 2 << 20;
/// The share of `query_bytes` one query's rows may take: an eighth, so eight
/// large queries fit at once. Picked, not measured.
const SLICE_SHARE: u64 = 8;
/// The smallest slice worth planning for.
const MIN_SLICE_BYTES: u64 = 16 << 10;
/// Segment files one query keeps open. Picked: a time-ordered read of one hour
/// touches at most a few segments, and a page of a sidecar costs far more.
const OPEN_FILES: usize = 16;
/// How often a read restarts after files vanish under it. Compaction takes
/// seconds, so meeting it this many times in one slice means something is wrong.
const RACE_RETRIES: u32 = 16;

/// Every bound a query enforces (SPEC §16).
///
/// `Default` is `from_config(&Config::default())`. Fields are public so a caller
/// can tighten one; `#[non_exhaustive]` so a new one is not a breaking change.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct QueryLimits {
    /// Results in one page: `Limits::max_query_page_samples`.
    pub page_results: usize,
    /// Approximate encoded bytes in one page. A single larger result still
    /// travels, alone.
    pub page_bytes: u64,
    /// Bytes of rows one time slice may hold before it is narrowed.
    pub slice_bytes: u64,
    /// Segment files kept open by one query, least recently used evicted.
    pub open_files: usize,
    /// Restarts of one read after its files vanish.
    pub race_retries: u32,
}

impl QueryLimits {
    /// Limits sized from `config`.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        Self {
            page_results: config.limits().max_query_page_samples,
            page_bytes: PAGE_BYTES,
            slice_bytes: (config.memory().query_bytes / SLICE_SHARE).max(MIN_SLICE_BYTES),
            open_files: OPEN_FILES,
            race_retries: RACE_RETRIES,
        }
    }
}

impl Default for QueryLimits {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}
