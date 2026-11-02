//! Where things live on disk: database → stream directory → segment files.
//!
//! ```text
//! <database>/streams/<stream id, 16 hex digits>/
//!     0000000000000000-g0000.nseg     a segment
//!     0000000000000001-g0000.nseg
//!     0000000000000000-g0001.nseg     a compaction output covering 0..=1
//!     quarantine/                     bytes recovery moved aside
//! ```
//!
//! Listing reads only each segment's 48-byte header.

use std::fs;
use std::path::{Path, PathBuf};

use norn_core::StreamId;

use crate::error::{Result, StorageError};
use crate::format::SegmentHeader;
use crate::fsio::Source;
use crate::segment::SegmentName;
use crate::segment::load::read_header;

/// The directory of `stream` under database root `root`.
#[must_use]
pub fn stream_dir(root: &Path, stream: StreamId) -> PathBuf {
    root.join("streams").join(format!("{:016x}", stream.get()))
}

/// A segment file and its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentEntry {
    /// The segment file.
    pub path: PathBuf,
    /// Its verified header.
    pub header: SegmentHeader,
}

impl SegmentEntry {
    /// Whether `self` holds every sample of `other`: a higher generation
    /// whose sequence range covers `other`'s.
    #[must_use]
    pub fn supersedes(&self, other: &Self) -> bool {
        let (a, b) = (&self.header, &other.header);
        a.generation > b.generation && a.first_seq <= b.first_seq && b.last_seq <= a.last_seq
    }
}

/// Every segment file in `dir` (`.tmp` and foreign files ignored), with
/// its header, sorted by name (sequence, then generation).
///
/// # Errors
/// [`StorageError::Io`]; [`StorageError::Corrupt`] or
/// [`StorageError::UnsupportedVersion`] for a header that does not verify
/// (run recovery first, which quarantines such files).
pub fn list_segments(dir: &Path) -> Result<Vec<SegmentEntry>> {
    let mut paths = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| StorageError::io("list", dir, e))?;
    for entry in entries {
        let path = entry.map_err(|e| StorageError::io("list", dir, e))?.path();
        if let Some(name) = SegmentName::parse(&path) {
            paths.push((name, path));
        }
    }
    paths.sort();
    let mut out = Vec::with_capacity(paths.len());
    for (_, path) in paths {
        let header = read_header(&mut Source::open(&path)?)?;
        out.push(SegmentEntry { path, header });
    }
    Ok(out)
}

/// The segments a reader should use: [`list_segments`] minus those
/// superseded by a committed compaction output.
///
/// Cost: O(n²) header comparisons over n segments, about 77 million for a
/// year of hourly segments; trivial next to reading them.
///
/// # Errors
/// As [`list_segments`].
pub fn live_segments(dir: &Path) -> Result<Vec<SegmentEntry>> {
    let all = list_segments(dir)?;
    let live = all
        .iter()
        .filter(|s| !all.iter().any(|t| t.supersedes(s)))
        .cloned()
        .collect();
    Ok(live)
}

/// The segments in `all` superseded by another, each with its superseder.
pub(crate) fn superseded(all: &[SegmentEntry]) -> Vec<(&SegmentEntry, &SegmentEntry)> {
    all.iter()
        .filter_map(|s| all.iter().find(|t| t.supersedes(s)).map(|t| (s, t)))
        .collect()
}
