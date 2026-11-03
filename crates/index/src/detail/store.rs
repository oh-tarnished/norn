//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Cached reads of a sidecar, falling back to the segment's own directory.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use norn_core::StreamId;
use norn_storage::format::frame::ChunkMeta;

use super::{DetailHead, decode_head, decode_page, encode_detail, head_len};
use crate::build::read_directory;
use crate::cache::SegmentKey;
use crate::error::{IndexError, Result};
use crate::record::SegmentRecord;
use crate::shared::Shared;
use crate::stats::add;

/// The cached head of `record`'s sidecar, loading or repairing it on a miss.
///
/// # Errors
/// [`IndexError::SegmentGone`] when sidecar and segment are both unusable.
pub(crate) fn read_head(
    shared: &Shared,
    stream: StreamId,
    record: &SegmentRecord,
) -> Result<Arc<DetailHead>> {
    let key = key_of(stream, record);
    if let Some(h) = shared.cache.head(key) {
        add(&shared.counters.cache_hits, 1);
        return Ok(h);
    }
    add(&shared.counters.cache_misses, 1);
    let path = shared.detail_path(stream, record.name());
    let len = head_len(record.chunks as usize);
    let head =
        read_range(shared, &path, 0, len).and_then(|bytes| decode_head(&bytes, stream, record));
    let head = match head {
        Some(h) => h,
        None => {
            let bytes = repair(shared, stream, record)?;
            let slice = bytes.get(..len).unwrap_or_default();
            decode_head(slice, stream, record).ok_or_else(|| gone(stream, record))?
        }
    };
    let head = Arc::new(head);
    shared.cache.put_head(key, Arc::clone(&head));
    Ok(head)
}

/// The cached page `page` of `record`'s sidecar, loading or repairing it on a
/// miss. The result is shared, so holding it past the call is cheap.
///
/// # Errors
/// As [`read_head`].
pub(crate) fn read_page(
    shared: &Shared,
    stream: StreamId,
    record: &SegmentRecord,
    head: &DetailHead,
    page: usize,
) -> Result<Arc<[ChunkMeta]>> {
    let key = key_of(stream, record);
    let id = u32::try_from(page).unwrap_or(u32::MAX - 1);
    if let Some(p) = shared.cache.page(key, id) {
        add(&shared.counters.cache_hits, 1);
        return Ok(p);
    }
    add(&shared.counters.cache_misses, 1);
    let (at, len) = head.page_range(page).ok_or_else(|| gone(stream, record))?;
    let chunks = record.chunks as usize;
    let path = shared.detail_path(stream, record.name());
    let decoded =
        read_range(shared, &path, at, len).and_then(|bytes| decode_page(&bytes, chunks, page));
    let decoded = match decoded {
        Some(d) => d,
        None => {
            let bytes = repair(shared, stream, record)?;
            let start = usize::try_from(at).unwrap_or(usize::MAX);
            let slice = bytes
                .get(start..start.saturating_add(len))
                .unwrap_or_default();
            decode_page(slice, chunks, page).ok_or_else(|| gone(stream, record))?
        }
    };
    let decoded: Arc<[ChunkMeta]> = Arc::from(decoded);
    shared.cache.put_page(key, id, Arc::clone(&decoded));
    Ok(decoded)
}

fn key_of(stream: StreamId, record: &SegmentRecord) -> SegmentKey {
    SegmentKey {
        stream,
        segment: record.name(),
        file_len: record.file_len,
    }
}

fn gone(stream: StreamId, record: &SegmentRecord) -> IndexError {
    IndexError::SegmentGone {
        stream,
        segment: record.name(),
    }
}

/// Opens `path` and reads `len` bytes at `at`; `None` for any failure, all of
/// which mean the same thing to the caller: repair.
fn read_range(shared: &Shared, path: &Path, at: u64, len: usize) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    add(&shared.counters.detail_opens, 1);
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = vec![0u8; len];
    f.read_exact(&mut bytes).ok()?;
    add(&shared.counters.detail_bytes_read, len as u64);
    Some(bytes)
}

/// Re-encodes the sidecar from the segment's directory, and writes it back. A
/// failed write is counted, not returned: the bytes still answer this lookup.
///
/// Readers may call this concurrently. Temp names are unique and the rename is
/// atomic, so the worst outcome is writing identical bytes twice.
fn repair(shared: &Shared, stream: StreamId, record: &SegmentRecord) -> Result<Vec<u8>> {
    add(&shared.counters.detail_repairs, 1);
    let path = shared.segment_path(stream, record.name());
    let dir = read_directory(shared, &path).map_err(|e| match e {
        // Whatever the segment's state, this snapshot no longer describes it.
        IndexError::Storage(_) => gone(stream, record),
        other => other,
    })?;
    if dir.record != *record {
        return Err(gone(stream, record));
    }
    let mut chunks = dir.chunks;
    let bytes = encode_detail(stream, record, &mut chunks);
    let sidecar = shared.detail_path(stream, record.name());
    if write_detail(&sidecar, &bytes).is_err() {
        add(&shared.counters.detail_write_failures, 1);
    }
    Ok(bytes)
}

/// Writes a sidecar: unique temp file, then rename. Deliberately no fsync: it is
/// derived and checksummed, so a torn one after power loss is just a repair, and
/// a rebuild would otherwise spend its time in thousands of fsyncs.
///
/// # Errors
/// The first failing step; the temp file is removed on the way out.
pub(crate) fn write_detail(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}-{n}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(name);
    let done = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}
