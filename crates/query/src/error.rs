//! The query engine's typed errors. Callers match on variants, never on messages.

use std::fmt;
use std::sync::Arc;

use norn_core::{SignalId, StreamId, Timestamp};
use norn_index::IndexError;
use norn_storage::{SegmentName, StorageError};

use crate::filter::FilterError;
use crate::memory::Refused;

/// Where stored data failed its checks, enough for an operator to find it and for
/// a client to query around it.
///
/// Concurrency: owned plain data.
#[derive(Debug)]
pub struct ChunkDamage {
    /// The stream.
    pub stream: StreamId,
    /// The segment.
    pub segment: SegmentName,
    /// The frame's byte offset in the segment.
    pub offset: u64,
    /// The signal, from the index.
    pub signal: SignalId,
    /// The chunk's first timestamp.
    pub min_ts: Timestamp,
    /// The chunk's last timestamp.
    pub max_ts: Timestamp,
    /// What the read said.
    pub source: StorageError,
}

/// Why a query failed.
///
/// `#[non_exhaustive]`: match with a wildcard arm. Not `Clone`, since it may
/// carry an I/O error. `Send + Sync`.
#[derive(Debug)]
#[non_exhaustive]
pub enum QueryError {
    /// The sample filter does not parse.
    Filter(FilterError),
    /// A clause or a reduction cannot apply to a selected signal, such as
    /// `value > 1` on a string signal, or SUM on a boolean one.
    NotApplicable {
        /// The signal.
        signal: Arc<str>,
        /// What cannot apply.
        what: &'static str,
    },
    /// A resume position does not fit this query.
    BadPosition,
    /// A chunk failed its checksum or could not be decoded. Never skipped.
    Corrupt(Box<ChunkDamage>),
    /// The query budget could not cover the least this query needs at once, even
    /// after narrowing as far as one instant.
    Memory(Refused),
    /// One instant holds more than can be read at once: too many samples or too
    /// many chunks. Narrow the signals.
    TooLarge,
    /// A file kept vanishing under the query, past the retry bound.
    Raced,
    /// Reading a segment failed for a reason other than damage.
    Storage(StorageError),
    /// The index could not answer.
    Index(IndexError),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filter(e) => write!(f, "sample_filter: {e}"),
            Self::NotApplicable { signal, what } => write!(f, "{signal}: {what}"),
            Self::BadPosition => f.write_str("the resume position does not fit this query"),
            Self::Corrupt(d) => write!(
                f,
                "stream {} segment {} chunk at {}: {}",
                d.stream,
                d.segment.file_name(),
                d.offset,
                d.source
            ),
            Self::Memory(e) => write!(f, "{e}"),
            Self::TooLarge => f.write_str("one instant holds more than one read can take"),
            Self::Raced => f.write_str("segments kept changing under the query"),
            Self::Storage(e) => write!(f, "{e}"),
            Self::Index(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Filter(e) => Some(e),
            Self::Corrupt(d) => Some(&d.source),
            Self::Memory(e) => Some(e),
            Self::Storage(e) => Some(e),
            Self::Index(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Refused> for QueryError {
    fn from(e: Refused) -> Self {
        Self::Memory(e)
    }
}

/// Shorthand used throughout the crate.
pub type Result<T> = std::result::Result<T, QueryError>;
