//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The index's bounds, from the engine configuration.

use norn_core::Config;
use norn_storage::StorageOptions;

/// How many threads a full rebuild uses by default. Picked: a rebuild is mostly
/// reads and CRCs, and four is enough without starving ingest on a small box.
const DEFAULT_REBUILD_THREADS: usize = 4;

/// Every bound the index enforces (SPEC §16).
///
/// `Default` is `from_config(&Config::default())`. Fields are public so a caller
/// can tighten one; `#[non_exhaustive]` so a new one is not a breaking change.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexOptions {
    /// `MemoryBudgets::index_bytes`. A value rather than the runtime's `Budget`,
    /// because this crate may not import `norn-runtime`. The runtime passes the
    /// capacity of its index budget, which nothing else draws on.
    pub memory_budget_bytes: u64,
    /// Locators one [`crate::Plan`] may buffer. Default: twice
    /// `max_chunks_per_segment`, since a plan holds one segment's matches plus
    /// those of whatever overlaps it. At about 80 bytes each that is about 15
    /// MiB of query working memory at worst. Picked, not measured.
    pub max_plan_buffered: usize,
    /// Threads reading directories during a rebuild; at least one is always
    /// used. They are scoped: none outlives the call that started it.
    pub rebuild_threads: usize,
    /// The bounds segments are read under.
    pub storage: StorageOptions,
}

impl IndexOptions {
    /// Options sized from `config`.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let storage = StorageOptions::from_config(config);
        Self {
            memory_budget_bytes: config.memory().index_bytes,
            max_plan_buffered: storage.max_chunks_per_segment.saturating_mul(2),
            rebuild_threads: DEFAULT_REBUILD_THREADS,
            storage,
        }
    }
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}
