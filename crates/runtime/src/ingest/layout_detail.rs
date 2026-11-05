//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A segment's encodings, tallied chunk by chunk.

use std::collections::{BTreeMap, BTreeSet};

use norn_storage::{ChunkMeta, SegmentReader, StorageError};

/// One of the four columns every chunk stores, in body order.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ChunkColumn {
    /// Observation times.
    Timestamps,
    /// Values.
    Values,
    /// Quality codes.
    Qualities,
    /// Sequence numbers.
    Sequences,
}

const COLUMNS: [ChunkColumn; 4] = [
    ChunkColumn::Timestamps,
    ChunkColumn::Values,
    ChunkColumn::Qualities,
    ChunkColumn::Sequences,
];

/// The chunks of a segment whose `column` uses one codec at one compression.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ColumnEncoding {
    /// Which column.
    pub column: ChunkColumn,
    /// The codec's display name, such as `float64-gorilla`.
    pub codec: &'static str,
    /// The zstd level; 0 when stored uncompressed.
    pub zstd_level: u8,
    /// Chunks whose column is stored this way.
    pub chunks: u64,
    /// Those columns' bytes in the file.
    pub stored_bytes: u64,
    /// Those columns' bytes decompressed, still encoded.
    pub encoded_bytes: u64,
}

/// What reading a whole segment found.
///
/// Concurrency: owned plain data. Memory: one entry per distinct encoding,
/// at most a few dozen, and one per distinct signal while tallying.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SegmentDetail {
    /// Distinct signals with a chunk in the segment.
    pub signals: u64,
    /// The readable chunks' samples at their in-memory width.
    pub decoded_bytes: u64,
    /// Chunks that failed a checksum or did not decode.
    pub damaged_chunks: u64,
    /// The encodings in use, by column, then codec, then zstd level.
    pub encodings: Vec<ColumnEncoding>,
}

/// Whether `e` is damage to one chunk, which the tally counts and moves past,
/// rather than a failure to read the file at all.
fn is_damage(e: &StorageError) -> bool {
    matches!(
        e,
        StorageError::Corrupt(_) | StorageError::Codec { .. } | StorageError::TooLarge { .. }
    )
}

/// Reads each of `chunks` from `reader` and tallies its encodings.
///
/// # Errors
/// The first error that is not damage to a single chunk, such as an I/O error.
pub(super) fn detail(
    reader: &mut SegmentReader,
    chunks: &[ChunkMeta],
) -> Result<SegmentDetail, StorageError> {
    let mut out = SegmentDetail::default();
    let mut signals = BTreeSet::new();
    let mut tally: BTreeMap<(ChunkColumn, &'static str, u8), [u64; 3]> = BTreeMap::new();
    for meta in chunks {
        signals.insert(meta.signal);
        let layout = match reader.read_layout(meta) {
            Ok(layout) => layout,
            Err(e) if is_damage(&e) => {
                out.damaged_chunks += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        out.decoded_bytes += layout.decoded_bytes;
        for (column, s) in COLUMNS.into_iter().zip(layout.sections) {
            let key = (column, s.codec.name(), s.zstd_level.unwrap_or(0));
            let entry = tally.entry(key).or_default();
            entry[0] += 1;
            entry[1] += s.stored_bytes;
            entry[2] += s.encoded_bytes;
        }
    }
    out.signals = signals.len() as u64;
    out.encodings = tally
        .into_iter()
        .map(
            |((column, codec, zstd_level), [chunks, stored, encoded])| ColumnEncoding {
                column,
                codec,
                zstd_level,
                chunks,
                stored_bytes: stored,
                encoded_bytes: encoded,
            },
        )
        .collect();
    Ok(out)
}
