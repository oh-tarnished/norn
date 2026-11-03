//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a caller can see of, and do to, a running compaction.

/// A point in [`super::compact_observed`], reached in this order.
///
/// Each names the state on disk at that moment, which is what a crash there
/// leaves; `tests/compaction_crash.rs` and the daemon's kill tests cover each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CompactionPhase {
    /// A first input chunk is merged, so a partial `.tmp` exists, or is about to.
    /// Reported once per compaction.
    Merging,
    /// The output is sealed and fsynced as `.tmp`. Not committed: recovery
    /// removes it and the inputs are live.
    OutputWritten,
    /// The rename and its directory fsync are done. Committed: the output
    /// supersedes the inputs, all still on disk.
    Committed,
    /// One input is deleted, as each one is. Recovery removes the rest.
    InputRemoved,
    /// Every input is deleted and the directory fsynced. The index has not
    /// been told.
    InputsRemoved,
}

/// Watches a compaction, and may stop it.
///
/// Concurrency: called from the compacting thread only, but `Sync` so that a
/// cancelling thread can share it. Every method runs inline in a compaction
/// holding no lock, so a slow one slows that compaction and nothing else.
pub trait CompactionObserver: Sync {
    /// Reached `phase`. Default: nothing.
    fn phase(&self, phase: CompactionPhase) {
        let _ = phase;
    }

    /// One more input chunk was read and merged. Default: nothing.
    fn chunk_read(&self) {}

    /// Whether to give up. Asked before each input chunk, so it takes effect
    /// within one chunk's work, and only before the commit. Default: never.
    fn cancelled(&self) -> bool {
        false
    }
}

/// Watches nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unobserved;

impl CompactionObserver for Unobserved {}
