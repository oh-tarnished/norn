//! The middle of a compaction: what must be an input, and writing the output.

use std::fs;
use std::path::Path;

use norn_codec::compress::Compression;

use super::observer::{CompactionObserver, CompactionPhase};
use crate::chunk::Chunk;
use crate::error::{Result, StorageError};
use crate::format::SegmentHeader;
use crate::fsio::Source;
use crate::layout::SegmentEntry;
use crate::options::StorageOptions;
use crate::segment::load::read_header;
use crate::segment::{SegmentName, SegmentReader, SegmentSummary, SegmentWriter};

/// The live segments of `dir` whose sequence ranges reach `lo..=hi`.
///
/// Reads no file named above `hi`: the writer's open segment is one, and its
/// header may not have reached the disk yet. A segment superseding one of these
/// overlaps the same range, so judging only these is enough.
///
/// Cost: one header read per segment at or below `hi`.
pub(super) fn overlapping_live(dir: &Path, lo: u64, hi: u64) -> Result<Vec<SegmentEntry>> {
    let entries = fs::read_dir(dir).map_err(|e| StorageError::io("list", dir, e))?;
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| StorageError::io("list", dir, e))?.path();
        let Some(name) = SegmentName::parse(&path) else {
            continue;
        };
        if name.seq > hi {
            continue;
        }
        let header = read_header(&mut Source::open(&path)?)?;
        if header.last_seq >= lo {
            found.push(SegmentEntry { path, header });
        }
    }
    let live = found
        .iter()
        .filter(|s| !found.iter().any(|t| t.supersedes(s)))
        .cloned()
        .collect();
    Ok(live)
}

/// Writes the merged segment to `tmp`, sealed, and checks it holds exactly
/// as many samples as the inputs.
pub(super) fn write_merged(
    readers: &mut [SegmentReader],
    tmp: &Path,
    header: SegmentHeader,
    opts: &StorageOptions,
    observer: &dyn CompactionObserver,
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
    let mut started = false;
    for (_, i, j) in plan {
        if observer.cancelled() {
            return Err(StorageError::CompactionCancelled);
        }
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
        observer.chunk_read();
        if !started {
            started = true;
            observer.phase(CompactionPhase::Merging);
        }
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

/// Whether `b` may join `a`. Payloads are bounded too: merging a signal of large
/// strings up to `max_chunk_samples` would make a chunk no reader may read.
fn fits(a: &Chunk, b: &Chunk, opts: &StorageOptions) -> bool {
    let payload = a.values().payload_bytes() + b.values().payload_bytes();
    a.signal() == b.signal()
        && a.kind() == b.kind()
        && a.len() + b.len() <= opts.max_chunk_samples
        && payload <= u64::from(opts.max_chunk_bytes) / 2
}
