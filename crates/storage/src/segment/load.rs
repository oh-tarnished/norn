//! Low-level segment reads shared by the reader and recovery: the header,
//! the sealed directory, and one verified frame.

use crate::crc32c::crc32c;
use crate::error::{Bound, CorruptionKind, Result, StorageError};
use crate::format::directory::{self, dir_len};
use crate::format::frame::{self, ChunkMeta};
use crate::format::{FRAME_HEADER_LEN, HEADER_LEN, SegmentHeader, TRAILER_LEN};
use crate::fsio::Source;
use crate::options::StorageOptions;

/// Reads and checks the header.
pub(crate) fn read_header(src: &mut Source) -> Result<SegmentHeader> {
    let mut buf = [0u8; HEADER_LEN];
    if src.len() < HEADER_LEN as u64 {
        let len = src.len();
        return Err(StorageError::corrupt(
            src.path(),
            0,
            len,
            CorruptionKind::TornTail,
        ));
    }
    src.read_at(0, &mut buf)?;
    SegmentHeader::decode(&buf, src.path())
}

/// The sealed directory, or `None` when the file carries no seal marker.
pub(crate) fn load_directory(
    src: &mut Source,
    opts: &StorageOptions,
) -> Result<Option<Vec<ChunkMeta>>> {
    let len = src.len();
    let min = (HEADER_LEN + TRAILER_LEN) as u64 + dir_len(0);
    if len < min {
        return Ok(None);
    }
    let mut trailer = [0u8; TRAILER_LEN];
    src.read_at(len - TRAILER_LEN as u64, &mut trailer)?;
    let Some(at) = directory::decode_trailer(&trailer) else {
        return Ok(None);
    };
    let dir_end = len - TRAILER_LEN as u64;
    let bad = |src: &Source| {
        StorageError::corrupt(
            src.path(),
            at,
            dir_end.saturating_sub(at),
            CorruptionKind::Directory,
        )
    };
    if at < HEADER_LEN as u64 || at > dir_end.saturating_sub(dir_len(0)) {
        return Err(bad(src));
    }
    let mut head = [0u8; 8];
    src.read_at(at, &mut head)?;
    let count = directory::decode_count(&head).ok_or_else(|| bad(src))?;
    if count > opts.max_chunks_per_segment {
        let (len, max) = (count as u64, opts.max_chunks_per_segment as u64);
        return Err(StorageError::TooLarge {
            bound: Bound::SegmentChunks,
            len,
            max,
        });
    }
    if at + dir_len(count) != dir_end {
        return Err(bad(src));
    }
    let mut bytes = Vec::new();
    src.read_vec(
        at,
        usize::try_from(dir_len(count)).unwrap_or(usize::MAX),
        &mut bytes,
    )?;
    directory::decode(&bytes, src.path(), at, opts.max_chunks_per_segment).map(Some)
}

/// Reads `meta`'s whole frame into `buf` and verifies it: its header must
/// match `meta` and its body must match the recorded CRC.
pub(crate) fn read_frame(
    src: &mut Source,
    meta: &ChunkMeta,
    opts: &StorageOptions,
    buf: &mut Vec<u8>,
) -> Result<()> {
    let corrupt =
        |src: &Source, kind| StorageError::corrupt(src.path(), meta.offset, meta.frame_len(), kind);
    if meta.body_len > opts.max_chunk_bytes {
        let (len, max) = (u64::from(meta.body_len), u64::from(opts.max_chunk_bytes));
        return Err(StorageError::TooLarge {
            bound: Bound::ChunkBytes,
            len,
            max,
        });
    }
    if meta.offset.saturating_add(meta.frame_len()) > src.len() {
        return Err(corrupt(src, CorruptionKind::DirectoryMismatch));
    }
    src.read_vec(
        meta.offset,
        usize::try_from(meta.frame_len()).unwrap_or(usize::MAX),
        buf,
    )?;
    let Some(h) = frame::decode(buf, meta.offset) else {
        return Err(corrupt(src, CorruptionKind::FrameHeader));
    };
    if h.meta != *meta {
        return Err(corrupt(src, CorruptionKind::DirectoryMismatch));
    }
    let computed = crc32c(buf.get(FRAME_HEADER_LEN..).unwrap_or_default());
    if computed != h.body_crc {
        return Err(corrupt(
            src,
            CorruptionKind::BodyChecksum {
                stored: h.body_crc,
                computed,
            },
        ));
    }
    Ok(())
}
