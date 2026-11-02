//! Block compression envelope: an encoded block, optionally zstd-compressed.
//!
//! Layout: `kind: u8` (0 stored raw, 1 zstd), `level: u8` (0 when raw),
//! `raw_len: varint`, then the body. A block that zstd does not shrink is
//! stored raw, so already-dense encodings are never recompressed blindly
//! (SPEC §14) and reading them costs a copy, not a decompression.

use crate::error::{CodecError, Result};
use crate::limits::MAX_BLOCK_BYTES;
use crate::wire::{Cursor, put_varint};
use crate::zstd;

/// A zstd level in the owner-approved range 3-6 (SPEC §14 decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZstdLevel(u8);

impl ZstdLevel {
    /// Level 3, upstream zstd's own default and Norn's (see docs/reference/storage.md).
    pub const DEFAULT: Self = Self(3);

    /// The level, if it lies in 3..=6.
    pub fn new(level: u8) -> Option<Self> {
        (3..=6).contains(&level).then_some(Self(level))
    }

    /// The numeric level.
    pub fn get(self) -> u8 {
        self.0
    }
}

impl Default for ZstdLevel {
    /// [`ZstdLevel::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How [`seal`] compresses a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Compression {
    /// Store the encoded block as is.
    None,
    /// zstd at one level; raw if that does not shrink the block.
    Zstd(ZstdLevel),
    /// Try every level 3..=6 and keep the smallest. Four encodes per
    /// block: for compaction, not ingest.
    ZstdSmallest,
    /// Level 3, measured by the benchmarks as the best ratio per CPU on
    /// encoded sensor blocks. The default.
    #[default]
    Default,
}

/// What [`seal`] actually stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    /// The block, uncompressed.
    Raw,
    /// A zstd frame at this level.
    Zstd(ZstdLevel),
}

/// Wraps an encoded `block` in an envelope, appending to `out`, and reports
/// whether it was compressed.
///
/// Performance: one zstd encode per level tried; no copy of `block` beyond
/// the one into `out`.
///
/// # Errors
/// [`CodecError::TooLarge`] for a block above [`MAX_BLOCK_BYTES`].
pub fn seal(block: &[u8], how: Compression, out: &mut Vec<u8>) -> Result<Stored> {
    if block.len() > MAX_BLOCK_BYTES {
        let (len, max) = (block.len() as u64, MAX_BLOCK_BYTES as u64);
        return Err(CodecError::TooLarge {
            offset: 0,
            len,
            max,
        });
    }
    let levels: &[u8] = match how {
        Compression::None => &[],
        Compression::Zstd(l) => &[l.0][..],
        Compression::ZstdSmallest => &[3, 4, 5, 6],
        Compression::Default => &[3],
    };
    let best = levels
        .iter()
        .map(|l| (zstd::compress(block, *l), *l))
        .min_by_key(|(f, _)| f.len());
    let (kind, level, body) = match best {
        Some((frame, l)) if frame.len() < block.len() => (1u8, l, frame),
        _ => (0u8, 0u8, block.to_vec()),
    };
    out.extend([kind, level]);
    put_varint(out, block.len() as u64);
    out.extend_from_slice(&body);
    Ok(match kind {
        1 => Stored::Zstd(ZstdLevel(level)),
        _ => Stored::Raw,
    })
}

/// Unwraps an envelope, appending the encoded block to `out`.
///
/// # Errors
/// [`CodecError::Truncated`], [`CodecError::Corrupt`] for an unknown kind
/// or level, [`CodecError::TooLarge`] for a recorded length above
/// [`MAX_BLOCK_BYTES`], [`CodecError::Decompress`] for a bad frame, and
/// [`CodecError::TrailingBytes`] when a raw body is longer than recorded.
pub fn open(envelope: &[u8], out: &mut Vec<u8>) -> Result<Stored> {
    let mut cur = Cursor::new(envelope);
    let (kind, level) = (cur.u8()?, cur.u8()?);
    let len = cur.count(MAX_BLOCK_BYTES)?;
    match (kind, level) {
        (0, 0) => {
            out.extend_from_slice(cur.take(len)?);
            cur.finish()?;
            Ok(Stored::Raw)
        }
        (1, l) => {
            let level = ZstdLevel::new(l).ok_or(CodecError::Corrupt {
                offset: 1,
                reason: "zstd level",
            })?;
            out.extend(zstd::decompress(cur.rest(), len, cur.pos())?);
            Ok(Stored::Zstd(level))
        }
        _ => Err(CodecError::Corrupt {
            offset: 0,
            reason: "unknown envelope kind",
        }),
    }
}
