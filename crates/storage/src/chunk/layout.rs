//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a chunk body is made of: each column's codec, compression and sizes.
//!
//! For inspection tools, not reads: the body is decoded whole first, so a
//! layout is only ever reported for a chunk that would also read correctly.

use std::path::Path;

use norn_codec::compress::{self, Stored};
use norn_codec::{CodecId, peek_header};

use super::Column;
use super::decode::decode_body;
use crate::error::{CorruptionKind, Result, StorageError};
use crate::format::Le;
use crate::format::frame::ChunkMeta;

/// How one of a chunk's four columns is stored.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionLayout {
    /// The codec of its block.
    pub codec: CodecId,
    /// The zstd level it was compressed at; `None` when stored raw.
    pub zstd_level: Option<u8>,
    /// Its bytes in the file: the compression envelope, without the section's
    /// 4-byte length.
    pub stored_bytes: u64,
    /// The encoded block's bytes after decompression.
    pub encoded_bytes: u64,
}

/// How a chunk is stored: its timestamps, values, qualities and sequences, in
/// that order, and what its samples weigh decoded.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLayout {
    /// The four column sections, in body order.
    pub sections: [SectionLayout; 4],
    /// The samples at their in-memory width: 8 bytes of timestamp, 8 of
    /// sequence, 2 of quality code, and the value's own width, or its length
    /// for a string or bytes value.
    pub decoded_bytes: u64,
}

/// The fixed in-memory width of a value of `column`'s kind; 0 for strings and
/// bytes, whose payload is counted by [`Column::payload_bytes`].
fn value_width(column: &Column) -> u64 {
    match column {
        Column::Bool(_) => 1,
        Column::I32(_) | Column::U32(_) | Column::F32(_) | Column::Enum(_) => 4,
        Column::I64(_) | Column::U64(_) | Column::F64(_) => 8,
        Column::String(_) | Column::Bytes(_) => 0,
    }
}

/// Lays out a body whose CRC the caller has verified. Reads `body` in place
/// and copies nothing out of it but sizes.
///
/// # Errors
/// As decoding the body: [`StorageError::Codec`], or
/// [`StorageError::Corrupt`] with [`CorruptionKind::Malformed`].
pub(crate) fn layout_body(meta: &ChunkMeta, body: &[u8], path: &Path) -> Result<ChunkLayout> {
    let chunk = decode_body(meta, body, path)?;
    let values = chunk.values();
    let fixed = 8 + 8 + 2 + value_width(values);
    let decoded_bytes = fixed * chunk.len() as u64 + values.payload_bytes();
    let malformed = || {
        let kind = CorruptionKind::Malformed("section length");
        StorageError::corrupt(path, meta.offset, meta.frame_len(), kind)
    };
    let codec = |source| StorageError::Codec {
        path: path.to_path_buf(),
        offset: meta.offset,
        source,
    };
    let mut r = Le::new(body);
    let mut block = Vec::new();
    let mut section = || -> Result<SectionLayout> {
        let len = r.u32().ok_or_else(malformed)?;
        let envelope = r.take(len as usize).ok_or_else(malformed)?;
        block.clear();
        let stored = compress::open(envelope, &mut block).map_err(codec)?;
        let header = peek_header(&block).map_err(codec)?;
        Ok(SectionLayout {
            codec: header.codec,
            zstd_level: match stored {
                Stored::Raw => None,
                Stored::Zstd(level) => Some(level.get()),
            },
            stored_bytes: u64::from(len),
            encoded_bytes: block.len() as u64,
        })
    };
    let sections = [section()?, section()?, section()?, section()?];
    Ok(ChunkLayout {
        sections,
        decoded_bytes,
    })
}
