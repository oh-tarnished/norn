//! The index's typed errors. Callers match on variants, never on messages.

use std::fmt;
use std::path::PathBuf;

use norn_core::StreamId;
use norn_storage::{SegmentName, StorageError};

/// Why an index operation failed.
///
/// A damaged index file is deliberately *not* here: that is a rebuild, reported
/// through [`crate::IndexReport`] or [`crate::IndexStats`]. These are what the
/// caller must act on. `#[non_exhaustive]`: match with a wildcard arm. Not
/// `Clone`, since it may carry an I/O error. `Send + Sync`.
#[derive(Debug)]
#[non_exhaustive]
pub enum IndexError {
    /// Reading a segment failed. Corruption here means recovery has not run
    /// (docs/reference/storage.md "Recovery").
    Storage(StorageError),
    /// A file under the index directory could not be listed or created.
    Io {
        /// What was being done.
        op: &'static str,
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Segment lists and tails alone would pass the memory budget. The update
    /// or open changed nothing. Seal or compact to reclaim room, or raise
    /// `index_bytes`; the data on disk is unaffected.
    OverBudget {
        /// Bytes the lists and tails would need.
        needed: u64,
        /// The budget.
        budget: u64,
    },
    /// The update took effect and answers are correct, but `summary.nidx`
    /// could not be written, so the next startup reads more directories. The
    /// write path should log this and carry on.
    PersistFailed(StorageError),
    /// A plan would buffer more locators than
    /// [`crate::IndexOptions::max_plan_buffered`], because that many chunks
    /// overlap in time. Narrow the range or the signals, and resume.
    PlanTooLarge {
        /// How many were buffered.
        buffered: usize,
        /// The bound.
        limit: usize,
    },
    /// A snapshot names a segment that no longer exists: compaction or
    /// retention removed it. Call [`crate::TemporalIndex::refresh`] and plan
    /// again from the last timestamp delivered.
    SegmentGone {
        /// The stream.
        stream: StreamId,
        /// The segment.
        segment: SegmentName,
    },
    /// A segment file sits in another stream's directory.
    WrongStream {
        /// The file.
        path: PathBuf,
        /// The stream its directory belongs to.
        expected: StreamId,
        /// The stream its header names.
        found: StreamId,
    },
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "index: {e}"),
            Self::Io { op, path, source } => {
                write!(f, "index: {op} {}: {source}", path.display())
            }
            Self::OverBudget { needed, budget } => {
                write!(f, "index: needs {needed} bytes, budget is {budget}")
            }
            Self::PersistFailed(e) => write!(f, "index applied but not persisted: {e}"),
            Self::PlanTooLarge { buffered, limit } => {
                write!(f, "plan buffered {buffered} locators, limit is {limit}")
            }
            Self::SegmentGone { stream, segment } => write!(
                f,
                "stream {stream}: segment {} is gone; refresh and plan again",
                segment.file_name()
            ),
            Self::WrongStream {
                path,
                expected,
                found,
            } => write!(
                f,
                "{}: holds stream {found}, directory is stream {expected}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(e) | Self::PersistFailed(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<StorageError> for IndexError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

/// Shorthand used throughout the crate.
pub type Result<T> = std::result::Result<T, IndexError>;
