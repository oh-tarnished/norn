//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What opening the index found, as typed values rather than log lines.

use norn_core::StreamId;

/// Why a `summary.nidx` was not used. None of these is an error: the index is
/// rebuilt from segment directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiscardReason {
    /// The file could not be read.
    Unreadable,
    /// It is shorter or longer than its count says, or too large for the budget.
    Malformed,
    /// Its magic is wrong.
    BadMagic,
    /// Its CRC does not match.
    BadChecksum,
    /// It was written by a newer Norn. An index file, unlike a segment, can
    /// simply be rebuilt, so this does not stop startup.
    NewerVersion {
        /// The version found.
        found: u16,
    },
    /// It names another stream.
    WrongStream,
}

/// What became of a stream's `summary.nidx` at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SummaryState {
    /// Read and verified.
    Loaded,
    /// There was none: a new database, or `index/` was deleted.
    Missing,
    /// Present but unusable.
    Discarded(DiscardReason),
}

/// One stream's startup.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StreamReport {
    /// The stream.
    pub stream: StreamId,
    /// What became of its summary.
    pub summary: SummaryState,
    /// Segments trusted from the summary, by name and length, unread.
    pub reused: usize,
    /// Segments whose directories were read: new, changed, or all of them after
    /// a discarded summary.
    pub read: usize,
    /// Segments in the summary no longer on disk, or no longer live.
    pub dropped: usize,
    /// Unsealed segments served as in-memory tails.
    pub tails: usize,
    /// Orphan sidecars and temp files removed.
    pub orphans_removed: usize,
    /// Whether `summary.nidx` now matches. False when it could not be written:
    /// answers are still correct, and the next startup reads more directories.
    pub persisted: bool,
}

impl StreamReport {
    /// Whether every segment had to be read: a full rebuild.
    #[must_use]
    pub fn rebuilt(&self) -> bool {
        self.summary != SummaryState::Loaded
    }

    /// Whether the summary was exactly current.
    #[must_use]
    pub fn clean(&self) -> bool {
        self.summary == SummaryState::Loaded && self.read == 0 && self.dropped == 0
    }
}

/// What [`crate::TemporalIndex::open`] found, one entry per stream directory.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct IndexReport {
    /// Per stream, in id order.
    pub streams: Vec<StreamReport>,
}
