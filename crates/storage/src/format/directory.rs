//! The chunk directory and trailer that seal a segment.
//!
//! Directory: magic `NDIR`, `count: u32`, `count` entries of
//! [`DIR_ENTRY_LEN`] bytes, then a CRC-32C of everything before it. Entry:
//! offset `u64`, body length `u32`, signal `u64`, earliest and latest
//! timestamp `i64`, sample count `u32`, value kind `u8`, flags `u8`,
//! reserved `u16`. Trailer, the file's last [`TRAILER_LEN`] bytes: the
//! directory's offset `u64`, then [`TRAILER_MAGIC`].
//!
//! The trailer is written last, after the directory, so a crash while
//! sealing leaves no valid trailer and the segment reads as unsealed: the
//! seal is all-or-nothing without a second file. The directory must also end
//! exactly where the trailer begins, which rejects a stale trailer.

use std::path::Path;

use norn_core::{SignalId, Timestamp, ValueKind};

use super::frame::ChunkMeta;
use super::{DIR_ENTRY_LEN, DIR_MAGIC, Le, TRAILER_LEN, TRAILER_MAGIC};
use crate::crc32c::crc32c;
use crate::error::{Bound, CorruptionKind, Result, StorageError};

/// Bytes of a directory holding `count` entries, CRC included.
#[must_use]
pub fn dir_len(count: usize) -> u64 {
    12 + (count as u64).saturating_mul(DIR_ENTRY_LEN as u64)
}

/// Appends the directory for `chunks` and the trailer to `out`; the
/// directory starts at file offset `dir_offset`.
pub(crate) fn encode(chunks: &[ChunkMeta], dir_offset: u64, out: &mut Vec<u8>) {
    let start = out.len();
    out.extend_from_slice(&DIR_MAGIC);
    out.extend_from_slice(
        &u32::try_from(chunks.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    for c in chunks {
        out.extend_from_slice(&c.offset.to_le_bytes());
        out.extend_from_slice(&c.body_len.to_le_bytes());
        out.extend_from_slice(&c.signal.get().to_le_bytes());
        out.extend_from_slice(&c.min_ts.as_nanos().to_le_bytes());
        out.extend_from_slice(&c.max_ts.as_nanos().to_le_bytes());
        out.extend_from_slice(&c.count.to_le_bytes());
        out.extend([c.kind.tag(), 0, 0, 0]);
    }
    let crc = crc32c(out.get(start..).unwrap_or_default());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&dir_offset.to_le_bytes());
    out.extend_from_slice(&TRAILER_MAGIC);
}

/// The directory offset recorded in a trailer, or `None` when `trailer` is
/// not a seal marker (the segment is unsealed or its seal is torn).
#[must_use]
pub fn decode_trailer(trailer: &[u8]) -> Option<u64> {
    let mut r = Le::new(trailer.get(..TRAILER_LEN)?);
    let offset = r.u64()?;
    (r.bytes::<8>()? == TRAILER_MAGIC).then_some(offset)
}

/// The entry count recorded in a directory's first eight bytes, or `None`
/// when they do not start a directory.
pub(crate) fn decode_count(head: &[u8]) -> Option<usize> {
    let mut r = Le::new(head);
    (r.bytes::<4>()? == DIR_MAGIC).then_some(())?;
    usize::try_from(r.u32()?).ok()
}

/// Parses a whole directory (from its magic through its CRC) read from
/// `path` at `dir_offset`. Copies into the returned vector.
///
/// # Errors
/// [`StorageError::TooLarge`] when it lists more than `max_entries`
/// (checked before allocating); [`StorageError::Corrupt`] with
/// [`CorruptionKind::Directory`] for bad framing, CRC, or an unknown kind.
pub(crate) fn decode(
    bytes: &[u8],
    path: &Path,
    dir_offset: u64,
    max_entries: usize,
) -> Result<Vec<ChunkMeta>> {
    let bad = || {
        StorageError::corrupt(
            path,
            dir_offset,
            bytes.len() as u64,
            CorruptionKind::Directory,
        )
    };
    let count = decode_count(bytes).ok_or_else(bad)?;
    if count > max_entries {
        let (len, max) = (count as u64, max_entries as u64);
        return Err(StorageError::TooLarge {
            bound: Bound::SegmentChunks,
            len,
            max,
        });
    }
    if bytes.len() as u64 != dir_len(count) {
        return Err(bad());
    }
    let (body, crc) = bytes.split_at(bytes.len().saturating_sub(4));
    if crc != crc32c(body).to_le_bytes() {
        return Err(bad());
    }
    let mut r = Le::new(body);
    r.take(8).ok_or_else(bad)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(entry(&mut r).ok_or_else(bad)?);
    }
    Ok(out)
}

fn entry(r: &mut Le<'_>) -> Option<ChunkMeta> {
    let offset = r.u64()?;
    let body_len = r.u32()?;
    let signal = SignalId::new(r.u64()?);
    let min_ts = Timestamp::from_nanos(r.i64()?);
    let max_ts = Timestamp::from_nanos(r.i64()?);
    let count = r.u32()?;
    let kind = ValueKind::from_tag(r.u8()?)?;
    r.take(3)?;
    Some(ChunkMeta {
        offset,
        body_len,
        signal,
        kind,
        min_ts,
        max_ts,
        count,
    })
}
