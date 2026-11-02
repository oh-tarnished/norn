//! Retention hooks (SPEC §43): list what a policy *could* delete, and delete
//! what a caller then names.
//!
//! Nothing here decides. Retention is optional and explicit: a caller lists
//! candidates, shows or logs them, and deletes deliberately. Only whole sealed
//! segments are ever listed, because a segment is the unit that can be removed
//! without rewriting anything. An unsealed segment is never listed (it may be
//! the one being written, and has not been verified), and neither is an empty
//! one: holding no samples, it has no time to fall before a cutoff.

use std::path::Path;

use norn_core::Timestamp;

use crate::error::Result;
use crate::layout::live_segments;
use crate::options::StorageOptions;
use crate::platform::sync_dir;
use crate::segment::{SegmentReader, SegmentSummary};

/// Summaries of `dir`'s sealed live segments, in sequence order.
fn sealed_summaries(dir: &Path, opts: &StorageOptions) -> Result<Vec<SegmentSummary>> {
    let mut out = Vec::new();
    for entry in live_segments(dir)? {
        let reader = SegmentReader::open(&entry.path, *opts)?;
        if reader.is_sealed() {
            out.push(reader.summary());
        }
    }
    Ok(out)
}

/// The sealed live segments of stream directory `dir` whose every sample is
/// strictly before `cutoff`, oldest first. A segment with one sample at or
/// after `cutoff` is not listed: partial deletion is never offered.
///
/// Cost: reads each segment's header and directory, no chunk bodies.
///
/// # Errors
/// [`crate::StorageError::Io`], or a corrupt segment (run recovery first).
pub fn segments_before(
    dir: &Path,
    cutoff: Timestamp,
    opts: &StorageOptions,
) -> Result<Vec<SegmentSummary>> {
    let mut all = sealed_summaries(dir, opts)?;
    all.retain(|s| s.time_range.is_some_and(|(_, max)| max < cutoff));
    Ok(all)
}

/// The oldest sealed live segments that would have to go for `dir`'s live
/// segments to total at most `max_bytes` (unsealed ones count towards the
/// total but are never listed), oldest first. Empty when already under.
///
/// # Errors
/// As [`segments_before`].
pub fn segments_over_size(
    dir: &Path,
    max_bytes: u64,
    opts: &StorageOptions,
) -> Result<Vec<SegmentSummary>> {
    let mut total = 0u64;
    for entry in live_segments(dir)? {
        let len = std::fs::metadata(&entry.path)
            .map_err(|e| crate::StorageError::io("stat", &entry.path, e))?
            .len();
        total = total.saturating_add(len);
    }
    let mut out = Vec::new();
    for s in sealed_summaries(dir, opts)? {
        if total <= max_bytes {
            break;
        }
        total = total.saturating_sub(s.bytes);
        out.push(s);
    }
    Ok(out)
}

/// Deletes `paths`, in order, from stream directory `dir`, then fsyncs it.
///
/// The caller has decided these go; this crate never does (SPEC §43). Pass them
/// oldest first: a crash part-way leaves a stream that has lost only its oldest
/// segments, which is a retention pass that has not finished. A file already
/// gone counts as deleted, so repeating this is safe.
///
/// Concurrency: a reader holding a deleted file open keeps reading it; one that
/// has not opened it gets `NotFound`, which `norn-query` treats as a race.
///
/// # Errors
/// [`crate::StorageError::Io`]; files before the failure are gone.
pub fn remove_segments(dir: &Path, paths: &[std::path::PathBuf]) -> Result<()> {
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(crate::StorageError::io("remove", path, e)),
        }
    }
    sync_dir(dir)
}
