//! Compaction, format side (SPEC §42): merge small chunks into large ones.

use std::fs;
use std::path::{Path, PathBuf};

use norn_codec::compress::Compression;

use crate::chunk::Chunk;
use crate::error::{Result, StorageError};
use crate::format::SegmentHeader;
use crate::fsio::tmp_path;
use crate::layout::live_segments;
use crate::options::StorageOptions;
use crate::platform::sync_dir;
use crate::segment::{SegmentName, SegmentReader, SegmentSummary, SegmentWriter};

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
/// [`Compression::ZstdSmallest`]. Every sample is preserved exactly, in its
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
/// # Errors
/// [`StorageError::CompactionRefused`] for unmet preconditions (nothing
/// changed); any read, decode or write error before the commit (the inputs
/// stay live); an I/O error while deleting inputs after the commit (they are
/// superseded, and recovery finishes the job). A stale `.tmp` from an earlier
/// crash makes the create fail, so run recovery first.
pub fn compact(dir: &Path, inputs: &[PathBuf], opts: &StorageOptions) -> Result<CompactionOutcome> {
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
    let missing = live_segments(dir)?.into_iter().any(|s| {
        s.header.last_seq >= lo
            && s.header.first_seq <= hi
            && !names.contains(&s.path.file_name().map(ToOwned::to_owned))
    });
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
    let written = write_merged(&mut readers, &tmp, header, opts);
    let output = match written {
        Ok(o) => o,
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    };
    fs::rename(&tmp, &path).map_err(|e| StorageError::io("rename", &tmp, e))?;
    sync_dir(dir)?;
    let chunks_before = readers.iter().map(|r| r.chunks().len()).sum();
    drop(readers);
    let mut bytes_before = 0;
    for p in inputs {
        bytes_before += fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        fs::remove_file(p).map_err(|e| StorageError::io("remove", p, e))?;
    }
    sync_dir(dir)?;
    let output = SegmentSummary { path, ..output };
    Ok(CompactionOutcome {
        output,
        removed: inputs.to_vec(),
        chunks_before,
        bytes_before,
    })
}

/// Writes the merged segment to `tmp`, sealed, and checks it holds exactly
/// as many samples as the inputs.
fn write_merged(
    readers: &mut [SegmentReader],
    tmp: &Path,
    header: SegmentHeader,
    opts: &StorageOptions,
) -> Result<SegmentSummary> {
    let mut plan: Vec<(u64, usize, usize)> = Vec::new();
    for (i, r) in readers.iter().enumerate() {
        plan.extend(
            r.chunks()
                .iter()
                .enumerate()
                .map(|(j, c)| (c.signal.get(), i, j)),
        );
    }
    plan.sort();
    let expected: u64 = readers
        .iter()
        .flat_map(|r| r.chunks())
        .map(|c| u64::from(c.count))
        .sum();
    let mut w = SegmentWriter::create_at(tmp.to_path_buf(), header, *opts)?;
    let mut acc: Option<Chunk> = None;
    for (_, i, j) in plan {
        let r = readers.get_mut(i).ok_or(StorageError::Sealed)?;
        let meta = *r.chunks().get(j).ok_or(StorageError::Sealed)?;
        let chunk = r.read_chunk(&meta)?;
        acc = match acc.take() {
            Some(mut a) if fits(&a, &chunk, opts) => {
                a.extend(&chunk)?;
                Some(a)
            }
            Some(a) => {
                w.append_with(&a, Compression::ZstdSmallest)?;
                Some(chunk)
            }
            None => Some(chunk),
        };
    }
    if let Some(a) = acc {
        w.append_with(&a, Compression::ZstdSmallest)?;
    }
    let out = w.seal()?;
    if out.samples != expected {
        return Err(StorageError::CompactionRefused(
            "output sample count differs from inputs",
        ));
    }
    Ok(out)
}

fn fits(a: &Chunk, b: &Chunk, opts: &StorageOptions) -> bool {
    a.signal() == b.signal() && a.kind() == b.kind() && a.len() + b.len() <= opts.max_chunk_samples
}
