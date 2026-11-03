//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! From segment files to index entries: one directory, or many in parallel.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_core::StreamId;
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{SegmentHeader, SegmentReader};

use crate::detail::{encode_detail, write_detail};
use crate::error::{IndexError, Result};
use crate::record::SegmentRecord;
use crate::shared::Shared;
use crate::stats::add;
use crate::tail::TailLog;

/// A segment's directory, read.
#[derive(Debug)]
pub(crate) struct Directory {
    pub(crate) header: SegmentHeader,
    pub(crate) record: SegmentRecord,
    pub(crate) chunks: Vec<ChunkMeta>,
    pub(crate) sealed: bool,
}

/// What one segment file contributes to the index.
#[derive(Debug)]
pub(crate) enum Scanned {
    /// A sealed segment, whose sidecar now exists.
    Sealed(SegmentRecord),
    /// An unsealed one, served from memory.
    Unsealed(TailLog),
}

/// Reads the directory of the segment at `path`, or walks its frame headers if
/// it is unsealed. Memory: at most `max_chunks_per_segment` metas.
///
/// # Errors
/// [`IndexError::Storage`], for example a corrupt header: recovery has not run.
pub(crate) fn read_directory(shared: &Shared, path: &Path) -> Result<Directory> {
    let reader = SegmentReader::open(path, shared.options.storage)?;
    add(&shared.counters.segment_opens, 1);
    add(&shared.counters.segment_bytes_read, reader.bytes_read());
    let header = *reader.header();
    let chunks = reader.chunks().to_vec();
    let record = SegmentRecord::of(&header, &chunks, reader.summary().bytes);
    Ok(Directory {
        header,
        record,
        chunks,
        sealed: reader.is_sealed(),
    })
}

/// Indexes one segment file. A sealed segment gets its sidecar, on a best-effort
/// basis since a lookup repairs a missing one.
///
/// # Errors
/// As [`read_directory`]; [`IndexError::WrongStream`].
pub(crate) fn scan_segment(shared: &Shared, stream: StreamId, path: &Path) -> Result<Scanned> {
    let dir = read_directory(shared, path)?;
    if dir.header.stream != stream {
        return Err(IndexError::WrongStream {
            path: path.to_path_buf(),
            expected: stream,
            found: dir.header.stream,
        });
    }
    let Directory {
        record,
        mut chunks,
        sealed,
        ..
    } = dir;
    if !sealed {
        return Ok(Scanned::Unsealed(
            TailLog::new(record.name()).appended(&chunks),
        ));
    }
    let bytes = encode_detail(stream, &record, &mut chunks);
    if write_detail(&shared.detail_path(stream, record.name()), &bytes).is_err() {
        add(&shared.counters.detail_write_failures, 1);
    }
    Ok(Scanned::Sealed(record))
}

/// Indexes `paths` on up to `rebuild_threads` scoped threads, plus this one. The
/// result's order follows `paths`, whatever the scheduling, so a rebuild is
/// deterministic. Memory: one directory per thread, then one record per segment.
///
/// # Errors
/// The first failure in `paths` order; the other threads stop soon after.
pub(crate) fn scan_all(
    shared: &Shared,
    stream: StreamId,
    paths: &[PathBuf],
) -> Result<Vec<Scanned>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Result<Scanned>)>> = Mutex::new(Vec::new());
    let work = || {
        while let Some((i, path)) = {
            let i = next.fetch_add(1, Ordering::Relaxed);
            paths.get(i).map(|p| (i, p))
        } {
            let scanned = scan_segment(shared, stream, path);
            if scanned.is_err() {
                next.store(paths.len(), Ordering::Relaxed);
            }
            if let Ok(mut r) = results.lock() {
                r.push((i, scanned));
            }
        }
    };
    let helpers = shared.options.rebuild_threads.saturating_sub(1);
    std::thread::scope(|scope| {
        for _ in 0..helpers.min(paths.len().saturating_sub(1)) {
            // A failed spawn only means fewer helpers: this thread does the rest.
            let _ = std::thread::Builder::new().spawn_scoped(scope, work);
        }
        work();
    });
    let mut results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, r)| r).collect()
}
