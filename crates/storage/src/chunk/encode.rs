//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Chunk → frame bytes.

use std::path::PathBuf;

use norn_codec::compress::{self, Compression};
use norn_codec::{CodecError, boolean, bytes, enumeration, float32, float64, integer, timestamp};

use super::{Chunk, Column};
use crate::crc32c::crc32c;
use crate::error::{Bound, Result, StorageError};
use crate::format::FRAME_HEADER_LEN;
use crate::format::frame::{self, ChunkMeta};
use crate::options::StorageOptions;

/// A chunk's complete frame, encoded ahead of being written.
///
/// Encoding needs no file, so a caller can encode chunks in parallel and write
/// them in order ([`crate::StreamWriter::append_encoded`]); SPEC §17 asks for
/// exactly that split. Owned bytes, `Send`; memory is the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedChunk {
    pub(crate) frame: Vec<u8>,
    pub(crate) meta: ChunkMeta,
}

impl EncodedChunk {
    /// The frame's length in bytes, header included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frame.len()
    }

    /// Always false: a frame has a header. For clippy's `len` convention.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frame.is_empty()
    }

    /// The chunk's signal, time range and count. Its offset is meaningless
    /// until the chunk is written.
    #[must_use]
    pub fn meta(&self) -> &ChunkMeta {
        &self.meta
    }
}

impl Chunk {
    /// Encodes this chunk with `opts.compression`. Pure and CPU-bound: call it
    /// from any thread. Copies the columns into a new frame; the chunk is kept.
    ///
    /// # Errors
    /// As [`crate::SegmentWriter::append_with`], minus the I/O.
    pub fn encode(&self, opts: &StorageOptions) -> Result<EncodedChunk> {
        let mut frame = Vec::new();
        let meta = encode_frame(self, opts.compression, opts, 0, &mut frame)?;
        Ok(EncodedChunk { frame, meta })
    }
}

/// Appends `chunk`'s complete frame (header and body) to `out`, compressing
/// columns with `how`, and returns its metadata with `offset` set to
/// `offset`.
///
/// Memory: the encoded frame plus one column's scratch; bounded by
/// `opts.max_chunk_samples` and `opts.max_chunk_bytes`.
///
/// # Errors
/// [`StorageError::InvalidChunk`] for an empty chunk;
/// [`StorageError::TooLarge`] above either bound; [`StorageError::Codec`]
/// if a column cannot be encoded. `out` may hold a partial frame on error;
/// the writer discards it.
pub(crate) fn encode_frame(
    chunk: &Chunk,
    how: Compression,
    opts: &StorageOptions,
    offset: u64,
    out: &mut Vec<u8>,
) -> Result<ChunkMeta> {
    let (min_ts, max_ts) = chunk
        .time_range()
        .ok_or(StorageError::InvalidChunk("no samples"))?;
    let count = chunk.len();
    if count > opts.max_chunk_samples {
        let (len, max) = (count as u64, opts.max_chunk_samples as u64);
        return Err(StorageError::TooLarge {
            bound: Bound::ChunkSamples,
            len,
            max,
        });
    }
    let start = out.len();
    out.extend_from_slice(&[0u8; FRAME_HEADER_LEN]);
    let body_start = out.len();
    let mut block = Vec::new();
    let wrap = |source: CodecError| StorageError::Codec {
        path: PathBuf::new(),
        offset,
        source,
    };
    timestamp::encode(chunk.timestamps(), &mut block).map_err(wrap)?;
    section(&block, how, out).map_err(wrap)?;
    block.clear();
    values(chunk.values(), &mut block).map_err(wrap)?;
    section(&block, how, out).map_err(wrap)?;
    block.clear();
    let codes: Vec<u32> = chunk
        .qualities()
        .iter()
        .map(|q| u32::from(q.code()))
        .collect();
    enumeration::encode(&codes, &mut block).map_err(wrap)?;
    section(&block, how, out).map_err(wrap)?;
    block.clear();
    let seqs: Vec<i64> = chunk.sequences().iter().map(|s| *s as i64).collect();
    integer::encode(&seqs, &mut block).map_err(wrap)?;
    section(&block, how, out).map_err(wrap)?;
    let body = out.get(body_start..).unwrap_or_default();
    let body_len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    // Encrypting adds a tag, and the bound is on what reaches the file.
    let tag = opts.keyring.map_or(0, |_| crate::crypt::TAG_LEN as u32);
    if body_len.saturating_add(tag) > opts.max_chunk_bytes {
        out.truncate(start);
        let (len, max) = (
            u64::from(body_len) + u64::from(tag),
            u64::from(opts.max_chunk_bytes),
        );
        return Err(StorageError::TooLarge {
            bound: Bound::ChunkBytes,
            len,
            max,
        });
    }
    let meta = ChunkMeta {
        offset,
        body_len,
        signal: chunk.signal(),
        kind: chunk.kind(),
        min_ts,
        max_ts,
        count: u32::try_from(count).unwrap_or(u32::MAX),
    };
    let header = frame::encode(&meta, crc32c(body));
    if let Some(slot) = out.get_mut(start..body_start) {
        slot.copy_from_slice(&header);
    }
    Ok(meta)
}

/// Seals `block` and appends it as a `u32`-length-prefixed section.
fn section(block: &[u8], how: Compression, out: &mut Vec<u8>) -> norn_codec::Result<()> {
    let len_at = out.len();
    out.extend_from_slice(&[0u8; 4]);
    compress::seal(block, how, out)?;
    let len = u32::try_from(out.len() - len_at - 4).unwrap_or(u32::MAX);
    if let Some(slot) = out.get_mut(len_at..len_at + 4) {
        slot.copy_from_slice(&len.to_le_bytes());
    }
    Ok(())
}

/// Encodes the value column with the codec family for its kind.
fn values(col: &Column, out: &mut Vec<u8>) -> norn_codec::Result<()> {
    match col {
        Column::Bool(v) => boolean::encode(v, out).map(drop),
        Column::I32(v) => integer::encode(&widen(v.iter().map(|x| i64::from(*x))), out).map(drop),
        Column::I64(v) => integer::encode(v, out).map(drop),
        Column::U32(v) => integer::encode(&widen(v.iter().map(|x| i64::from(*x))), out).map(drop),
        Column::U64(v) => integer::encode(&widen(v.iter().map(|x| *x as i64)), out).map(drop),
        Column::F32(v) => float32::encode(v, out).map(drop),
        Column::F64(v) => float64::encode(v, out).map(drop),
        Column::String(v) => bytes::encode(v, out).map(drop),
        Column::Bytes(v) => bytes::encode(v, out).map(drop),
        Column::Enum(v) => enumeration::encode(v, out).map(drop),
    }
}

fn widen(it: impl Iterator<Item = i64>) -> Vec<i64> {
    it.collect()
}
