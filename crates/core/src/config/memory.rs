//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-subsystem memory budgets.

const MIB: u64 = 1024 * 1024;

/// How many bytes each subsystem may hold (SPEC §16).
///
/// A budget is a ceiling the subsystem enforces with backpressure, never a
/// preallocation: an idle daemon uses far less. Under pressure a subsystem
/// refuses or delays work (SPEC §18) rather than allocate past its budget.
///
/// `#[non_exhaustive]`: start from [`Default`] and set fields, so a new
/// subsystem's budget is not a breaking change.
///
/// `Default` (picked, not yet measured; stage 15 benchmarks revisit them):
/// total 512 MiB; ingest 64 MiB; chunk cache 128 MiB; index 64 MiB; query
/// 64 MiB; replay 64 MiB; IPC 32 MiB. The subsystems sum to 416 MiB, leaving
/// headroom under the total for everything unbudgeted (code, stacks,
/// allocator overhead).
///
/// Invariants (checked by [`crate::ConfigBuilder::build`]): every budget is
/// non-zero, and the subsystem budgets sum to at most `total_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MemoryBudgets {
    /// The whole process's budget; the subsystems must fit inside it.
    pub total_bytes: u64,
    /// Samples received but not yet encoded and persisted.
    pub ingest_bytes: u64,
    /// Decoded chunks kept for reuse by queries and replay.
    pub chunk_cache_bytes: u64,
    /// The in-memory part of the temporal index.
    pub index_bytes: u64,
    /// Working memory of in-flight queries, summed over all of them.
    pub query_bytes: u64,
    /// Read-ahead buffers of replay sessions, summed over all of them.
    pub replay_bytes: u64,
    /// Connection buffers and frames in flight on the IPC transports.
    pub ipc_bytes: u64,
}

impl MemoryBudgets {
    /// The per-subsystem budgets, by field name, for validation and reports.
    #[must_use]
    pub fn subsystems(&self) -> [(&'static str, u64); 6] {
        [
            ("ingest_bytes", self.ingest_bytes),
            ("chunk_cache_bytes", self.chunk_cache_bytes),
            ("index_bytes", self.index_bytes),
            ("query_bytes", self.query_bytes),
            ("replay_bytes", self.replay_bytes),
            ("ipc_bytes", self.ipc_bytes),
        ]
    }
}

impl Default for MemoryBudgets {
    fn default() -> Self {
        Self {
            total_bytes: 512 * MIB,
            ingest_bytes: 64 * MIB,
            chunk_cache_bytes: 128 * MIB,
            index_bytes: 64 * MIB,
            query_bytes: 64 * MIB,
            replay_bytes: 64 * MIB,
            ipc_bytes: 32 * MIB,
        }
    }
}
