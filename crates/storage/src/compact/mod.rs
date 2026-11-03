//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Compaction, format side (SPEC §42): merge small chunks into large ones.

mod merge;
mod observer;

pub use observer::{CompactionObserver, CompactionPhase, Unobserved};

use std::fs;
use std::path::{Path, PathBuf};

use self::merge::{overlapping_live, write_merged};
use crate::error::{Result, StorageError};
use crate::format::SegmentHeader;
use crate::fsio::tmp_path;
use crate::options::StorageOptions;
use crate::platform::sync_dir;
use crate::segment::{SegmentName, SegmentReader, SegmentSummary};

/// What a compaction did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionOutcome {
    /// The new segment.
    pub output: SegmentSummary,
    /// The input segments, now deleted.
    pub removed: Vec<PathBuf>,
    /// Chunks across the inputs.
    pub chunks_before: usize,
    /// File bytes across the inputs.
    pub bytes_before: u64,
}

/// Rewrites the sealed segments `inputs` of stream directory `dir` as one
/// segment in which each signal's chunks are merged, in order, into chunks
/// of up to `max_chunk_samples`, every column re-encoded with
/// [`norn_codec::compress::Compression::ZstdSmallest`]. Every sample is preserved exactly, in its
/// original order.
///
/// Crash safety: write new, then switch, then delete old.
/// 1. The output is written to `<name>.nseg.tmp`, sealed and fsynced. A
///    crash here leaves the inputs untouched; recovery removes the `.tmp`.
/// 2. The rename to `<name>.nseg` (then a directory fsync) is the commit.
///    The output has a higher generation and covers the inputs' sequence
///    range, so from this point it supersedes them.
/// 3. The inputs are deleted. A crash here leaves superseded inputs, which
///    readers ignore and recovery deletes.
///
/// Inputs must be sealed, from one stream, and must include every live
/// segment inside their sequence range (so the output supersedes only
/// them). A corrupt input chunk aborts the compaction before the commit:
/// it never compacts over damage.
///
/// Memory: one merged chunk (at most `max_chunk_samples` samples), one
/// input chunk, and the input directories. Ingest is not blocked: it writes
/// to newer segments, which are never inputs while open.
///
/// Concurrency: one compaction per stream directory at a time, which the
/// caller ensures; a writer may be appending to newer segments throughout.
///
/// # Errors
/// [`StorageError::CompactionRefused`] for unmet preconditions (nothing
/// changed); any read, decode or write error before the commit (the inputs
/// stay live); an I/O error while deleting inputs after the commit (they are
/// superseded, and recovery finishes the job).
pub fn compact(dir: &Path, inputs: &[PathBuf], opts: &StorageOptions) -> Result<CompactionOutcome> {
    compact_observed(dir, inputs, opts, &Unobserved)
}

/// [`compact`], telling `observer` how it goes and stopping before the commit
/// if it asks to.
///
/// # Errors
/// As [`compact`]; [`StorageError::CompactionCancelled`] when `observer` asked
/// to stop, which leaves the inputs live and no temporary file.
pub fn compact_observed(
    dir: &Path,
    inputs: &[PathBuf],
    opts: &StorageOptions,
    observer: &dyn CompactionObserver,
) -> Result<CompactionOutcome> {
    let refuse = |why| Err(StorageError::CompactionRefused(why));
    let mut readers = Vec::with_capacity(inputs.len());
    for p in inputs {
        readers.push(SegmentReader::open(p, *opts)?);
    }
    readers.sort_by_key(|r| r.header().first_seq);
    let Some(first) = readers.first() else {
        return refuse("no inputs");
    };
    let stream = first.header().stream;
    if readers
        .iter()
        .any(|r| !r.is_sealed() || r.header().stream != stream)
    {
        return refuse("inputs must be sealed segments of one stream");
    }
    let lo = readers
        .iter()
        .map(|r| r.header().first_seq)
        .min()
        .unwrap_or(0);
    let hi = readers
        .iter()
        .map(|r| r.header().last_seq)
        .max()
        .unwrap_or(0);
    let names: Vec<_> = inputs
        .iter()
        .map(|p| p.file_name().map(ToOwned::to_owned))
        .collect();
    let missing = overlapping_live(dir, lo, hi)?
        .into_iter()
        .any(|s| !names.contains(&s.path.file_name().map(ToOwned::to_owned)));
    if missing {
        return refuse("inputs leave out a live segment in their sequence range");
    }
    let max_gen = readers
        .iter()
        .map(|r| r.header().generation)
        .max()
        .unwrap_or(0);
    let Some(generation) = max_gen.checked_add(1) else {
        return refuse("generation overflow");
    };
    let mut header = SegmentHeader::new(stream, lo);
    header.last_seq = hi;
    header.generation = generation;
    let path = dir.join(SegmentName::of(&header).file_name());
    let tmp = tmp_path(&path);
    // One compaction per directory, so a file here is a failed rename's residue.
    let _ = fs::remove_file(&tmp);
    let written = write_merged(&mut readers, &tmp, header, opts, observer);
    let output = match written {
        Ok(o) => o,
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    };
    observer.phase(CompactionPhase::OutputWritten);
    fs::rename(&tmp, &path).map_err(|e| StorageError::io("rename", &tmp, e))?;
    sync_dir(dir)?;
    observer.phase(CompactionPhase::Committed);
    let chunks_before = readers.iter().map(|r| r.chunks().len()).sum();
    drop(readers);
    let mut bytes_before = 0;
    for p in inputs {
        bytes_before += fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        fs::remove_file(p).map_err(|e| StorageError::io("remove", p, e))?;
        observer.phase(CompactionPhase::InputRemoved);
    }
    sync_dir(dir)?;
    observer.phase(CompactionPhase::InputsRemoved);
    let output = SegmentSummary { path, ..output };
    Ok(CompactionOutcome {
        output,
        removed: inputs.to_vec(),
        chunks_before,
        bytes_before,
    })
}
