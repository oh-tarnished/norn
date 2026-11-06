//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The checks a segment file gets: read whole, chunk by chunk, within budget.

use std::path::Path;

use norn_codec::{MAX_BLOCK_BYTES, MAX_BLOCK_VALUES};
use norn_core::{SignalId, StreamId, Timestamp, ValueKind};
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{SegmentHeader, SegmentName, SegmentReader, StorageOptions};

use crate::measure;

/// The stream every target's files belong to.
pub const STREAM: StreamId = StreamId::new(7);

/// One chunk's read: its frame, then four columns at once, each one opened block
/// and its decoded values. Above this, a read is not bounded by its limits.
#[must_use]
pub fn chunk_budget(opts: &StorageOptions) -> usize {
    let values = MAX_BLOCK_VALUES * size_of::<Vec<u8>>();
    opts.max_chunk_bytes as usize + 4 * (2 * values + 2 * MAX_BLOCK_BYTES)
}

/// The name of a fresh segment file, which recovery insists on.
#[must_use]
pub fn file_name() -> String {
    SegmentName::of(&SegmentHeader::new(STREAM, 1)).file_name()
}

/// A meta for a chunk at `offset`, its fields from `head` (8 bytes or zeros).
#[must_use]
pub fn meta_from(head: &[u8], offset: u64, body_len: u32) -> ChunkMeta {
    let byte = |i: usize| head.get(i).copied().unwrap_or(0);
    let count = u32::from(u16::from_le_bytes([byte(1), byte(2)]));
    ChunkMeta {
        offset,
        body_len,
        signal: SignalId::new(u64::from(byte(3))),
        kind: ValueKind::from_tag(byte(0) % 16).unwrap_or(ValueKind::F64),
        min_ts: Timestamp::from_nanos(i64::from(byte(4))),
        max_ts: Timestamp::from_nanos(i64::from(byte(4)) + i64::from(byte(5))),
        count,
    }
}

/// Opens `path` and reads every chunk it lists; returns the chunks readable.
/// Whatever reads must be what its directory entry said it was.
///
/// # Panics
/// When a read breaks the contract: that is the finding.
pub fn read_all(path: &Path, opts: StorageOptions) -> usize {
    let (reader, peak) = measure(|| SegmentReader::open(path, opts));
    let dir = opts.max_chunks_per_segment * size_of::<ChunkMeta>();
    assert!(peak <= 2 * dir + (2 << 20), "opening held {peak} bytes");
    let Ok(mut reader) = reader else {
        return 0;
    };
    let _ = reader.summary();
    let mut good = 0;
    for meta in reader.chunks().to_vec() {
        let (chunk, peak) = measure(|| reader.read_chunk(&meta));
        let limit = chunk_budget(&opts);
        assert!(
            peak <= limit,
            "reading a chunk held {peak} bytes, above {limit}"
        );
        if let Ok(chunk) = chunk {
            assert_eq!(chunk.signal(), meta.signal, "a chunk of another signal");
            assert_eq!(chunk.kind(), meta.kind, "a chunk of another kind");
            good += usize::from(chunk.samples().count() == chunk.len());
        }
    }
    good
}
