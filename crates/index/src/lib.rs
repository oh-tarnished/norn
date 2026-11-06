//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's temporal index (SPEC §21): which chunks hold this signal at this time?
//!
//! # Layer contract
//!
//! - **Owns** signal → chunks, time range → chunks, stream → chunks, and
//!   timestamp → nearest chunk at or before; the `summary.nidx` and `*.ndet`
//!   files under `<database>/index/`; and the in-memory tail of unsealed
//!   segments. docs/reference/storage.md "Temporal index" has the byte tables.
//! - **Must not** index individual samples (SPEC §21), hold state that
//!   cannot be rebuilt from segment directories, know any domain, or read a
//!   clock. **Entity → signals is not here**: `norn-metadata` owns it.
//! - **May import** `norn-core` and `norn-storage` only, so it cannot name the
//!   runtime's `Budget`: the budget arrives as a value in [`IndexOptions`].
//! - **Spans**: none yet; findings are returned as typed values
//!   ([`IndexReport`], [`IndexStats`]).
//!
//! # Shape
//!
//! Three tiers, because 140 million chunks a year cannot sit in 64 MiB: a
//! [`SegmentRecord`] per segment in memory, a per-signal chunk directory per
//! segment in a sidecar file behind a bounded page cache, and the segment
//! itself, which a lookup never opens.
//!
//! # Concurrency
//!
//! Many readers, one logical updater. [`TemporalIndex::snapshot`] clones an
//! `Arc` under a lock held for that clone alone; a [`Snapshot`] is immutable
//! and never blocks an update. Updates do their I/O outside any lock and swap
//! the pointer. See [`TemporalIndex`] for what a reader sees during compaction.
//!
//! # Failure
//!
//! The index is derived. A missing, stale or corrupt file triggers a rebuild,
//! is reported, and never loses data. Nothing panics on malformed input.
//!
//! # Memory
//!
//! Segment lists and tails are charged against
//! [`IndexOptions::memory_budget_bytes`] and the page cache gets what remains.
//! A [`Plan`] buffers at most [`IndexOptions::max_plan_buffered`] locators.

#![forbid(unsafe_code)]

mod build;
mod cache;
mod detail;
mod error;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
mod index;
mod live;
mod locator;
mod nearest;
mod options;
mod plan;
mod reconcile;
mod record;
mod report;
mod shared;
mod signals;
mod signals_file;
mod snapshot;
mod stats;
mod summary;
mod tail;
mod updates;
mod view;
mod wire;

pub use error::{IndexError, Result};
pub use index::TemporalIndex;
pub use locator::ChunkLocator;
pub use options::IndexOptions;
pub use plan::{Plan, PlanRequest, SignalFilter};
pub use record::SegmentRecord;
pub use report::{DiscardReason, IndexReport, StreamReport, SummaryState};
pub use signals::SignalSpan;
pub use snapshot::Snapshot;
pub use stats::IndexStats;
