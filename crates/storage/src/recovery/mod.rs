//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Crash recovery: verify segments, detect torn and corrupt regions, and
//! report every finding as a typed event (SPEC §20).
//!
//! Nothing is ever silently discarded. Bytes recovery cuts from a segment are
//! first copied to the stream's `quarantine/` directory and fsynced; a
//! segment whose header is unusable is moved there whole. The only files
//! recovery deletes are ones whose every sample provably exists elsewhere:
//! an uncommitted compaction output (its inputs are intact) and a segment
//! superseded by a clean, committed compaction output.

mod quarantine;
mod scan;
mod segment;
mod stream;

pub use segment::recover_segment;
pub use stream::{recover_stream, recover_stream_vouched};

use std::path::PathBuf;

use crate::error::Corruption;
use crate::segment::SegmentSummary;

/// Name of the per-stream directory holding quarantined bytes.
pub const QUARANTINE_DIR: &str = "quarantine";

/// Whether recovery may change files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecoveryMode {
    /// Verify and report only; touch nothing (for `nornctl verify`).
    ReportOnly,
    /// Verify, report, and repair: quarantine and truncate torn tails, seal
    /// unsealed segments, quarantine unusable files, finish interrupted
    /// compactions. What the daemon runs on startup.
    #[default]
    Repair,
}

/// What recovery did about a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// Nothing ([`RecoveryMode::ReportOnly`]).
    Reported,
    /// The region stays in the file; reads of it keep failing with a typed
    /// error. Used for a corrupt chunk inside a sealed segment.
    LeftInPlace,
    /// The region was copied to `to`, and the segment no longer lists it.
    Quarantined {
        /// The quarantine file.
        to: PathBuf,
    },
    /// The region was copied to `to`, then cut from the end of the file.
    QuarantinedAndTruncated {
        /// The quarantine file.
        to: PathBuf,
    },
}

/// One thing recovery found, and what it did.
///
/// `#[non_exhaustive]`: match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecoveryEvent {
    /// A corrupt or torn region inside a segment.
    Corruption {
        /// Where and what.
        corruption: Corruption,
        /// What was done.
        action: Action,
    },
    /// The segment's header is unusable, so nothing in it can be trusted to
    /// be what it claims; the whole file was moved to `to` (or would be).
    SegmentQuarantined {
        /// Why.
        corruption: Corruption,
        /// Where it went; `None` in report-only mode.
        to: Option<PathBuf>,
    },
    /// An unsealed segment (crash before sealing) was sealed over its
    /// complete chunks.
    Sealed {
        /// The segment.
        path: PathBuf,
        /// Chunks the new directory lists.
        chunks: usize,
    },
    /// An uncommitted compaction output was removed; its inputs hold every
    /// sample. `removed` is false in report-only mode.
    UncommittedRemoved {
        /// The `.tmp` file.
        path: PathBuf,
        /// Whether it was deleted.
        removed: bool,
    },
    /// A segment superseded by a committed compaction output was removed
    /// (interrupted cleanup). `removed` is false in report-only mode.
    SupersededRemoved {
        /// The superseded segment.
        path: PathBuf,
        /// The compaction output holding its samples.
        by: PathBuf,
        /// Whether it was deleted.
        removed: bool,
    },
}

impl RecoveryEvent {
    /// Whether the event reports damaged data (as opposed to housekeeping).
    #[must_use]
    pub fn is_corruption(&self) -> bool {
        matches!(
            self,
            Self::Corruption { .. } | Self::SegmentQuarantined { .. }
        )
    }
}

/// The outcome of recovering one segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRecovery {
    /// The segment as it now stands; `None` if it was (or would be)
    /// quarantined whole.
    pub summary: Option<SegmentSummary>,
    /// Every finding, in file order.
    pub events: Vec<RecoveryEvent>,
}

impl SegmentRecovery {
    /// Whether nothing was found.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.events.is_empty()
    }
}

/// What recovering a stream found: the report the daemon exposes (SPEC §34
/// `recovery_events`, `corruption_events`).
///
/// Memory: one summary per segment plus the events, each bounded by the
/// segment's chunk count. Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryReport {
    /// Every live segment after recovery, in sequence order.
    pub segments: Vec<SegmentSummary>,
    /// Every finding, segment by segment, then stream housekeeping.
    pub events: Vec<RecoveryEvent>,
    /// Segments whose every chunk was read and checked.
    pub verified_segments: u64,
    /// Segments a clean shutdown vouched for, whose chunks were not read.
    pub vouched_segments: u64,
}

impl RecoveryReport {
    /// Events that report damaged data.
    pub fn corruptions(&self) -> impl Iterator<Item = &RecoveryEvent> {
        self.events.iter().filter(|e| e.is_corruption())
    }

    /// Whether recovery found nothing at all.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.events.is_empty()
    }
}
