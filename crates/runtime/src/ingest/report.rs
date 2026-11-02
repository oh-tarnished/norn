//! What startup recovery reports.

use std::fmt::Debug;

use norn_storage::RecoveryReport;

/// The most findings kept as text; the counts always cover all of them.
const MAX_FINDINGS: usize = 64;

/// What startup recovery found, over every stream.
///
/// Concurrency: owned plain data. Memory: at most 64 strings of findings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct IngestRecovery {
    /// Streams examined.
    pub streams: u64,
    /// Findings of any kind.
    pub events: u64,
    /// Findings that report damaged data.
    pub corruptions: u64,
    /// Batches read back from journals.
    pub replayed_batches: u64,
    /// Samples moved from journals into segments.
    pub replayed_samples: u64,
    /// Journal samples already stored, and not written again.
    pub duplicate_samples: u64,
    /// Findings as text, for logs only.
    pub findings: Vec<String>,
    /// What opening the temporal index found.
    pub index: IndexSummary,
    /// Streams a clean shutdown vouched for whole, and so not read at all.
    pub clean_streams: u64,
    /// Segments whose every chunk was read and checked.
    pub verified_segments: u64,
    /// Segments taken on a clean shutdown's word, chunks unread.
    pub vouched_segments: u64,
}

/// [`norn_index::IndexReport`], totalled, so the server need not import the index.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexSummary {
    /// Streams examined.
    pub streams: u64,
    /// Streams whose summary had to be rebuilt.
    pub rebuilt_streams: u64,
    /// Segments trusted from a summary.
    pub reused_segments: u64,
    /// Segments whose directories were read.
    pub read_segments: u64,
    /// Unsealed segments served from memory.
    pub tail_segments: u64,
    /// Streams whose summary could not be written.
    pub unpersisted_streams: u64,
}

impl From<&norn_index::IndexReport> for IndexSummary {
    fn from(report: &norn_index::IndexReport) -> Self {
        let mut out = Self::default();
        for s in &report.streams {
            out.streams += 1;
            out.rebuilt_streams += u64::from(s.rebuilt());
            out.reused_segments += s.reused as u64;
            out.read_segments += s.read as u64;
            out.tail_segments += s.tails as u64;
            out.unpersisted_streams += u64::from(!s.persisted);
        }
        out
    }
}

impl IngestRecovery {
    pub(super) fn absorb(&mut self, report: &RecoveryReport) {
        self.events += report.events.len() as u64;
        self.corruptions += report.corruptions().count() as u64;
        for event in &report.events {
            self.note(event);
        }
    }

    pub(super) fn note(&mut self, finding: &impl Debug) {
        if self.findings.len() < MAX_FINDINGS {
            self.findings.push(format!("{finding:?}"));
        }
    }
}
