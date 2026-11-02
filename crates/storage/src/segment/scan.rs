//! Walking frame headers when a segment has no (valid) directory.

use crate::error::{Bound, Result, StorageError};
use crate::format::FRAME_HEADER_LEN;
use crate::format::frame::{self, ChunkMeta};
use crate::fsio::Source;
use crate::options::StorageOptions;

/// Walks chunk frame headers from `start` up to `end`, skipping bodies
/// unread, and returns the frames found and the offset where the walk
/// stopped. The walk stops at the first header that is not valid (bad
/// magic or CRC), whose body would run past `end`, or whose body exceeds
/// `max_chunk_bytes`: everything from there on is an unverified tail.
///
/// This is the read path's view of an unsealed segment (being written, or
/// left by a crash). Bodies are not checksummed here; each is verified when
/// read. Recovery verifies bodies and resynchronises past damage instead.
///
/// Cost: one 48-byte read per chunk. Memory: the returned list, bounded by
/// `max_chunks_per_segment`.
///
/// # Errors
/// [`StorageError::Io`]; [`StorageError::TooLarge`] past
/// `max_chunks_per_segment` frames.
pub(crate) fn scan_headers(
    src: &mut Source,
    start: u64,
    end: u64,
    opts: &StorageOptions,
) -> Result<(Vec<ChunkMeta>, u64)> {
    let mut chunks = Vec::new();
    let mut at = start;
    let mut buf = [0u8; FRAME_HEADER_LEN];
    while end.saturating_sub(at) >= FRAME_HEADER_LEN as u64 {
        src.read_at(at, &mut buf)?;
        let Some(h) = frame::decode(&buf, at) else {
            break;
        };
        let fits = at.saturating_add(h.meta.frame_len()) <= end;
        if !fits || h.meta.body_len > opts.max_chunk_bytes {
            break;
        }
        if chunks.len() >= opts.max_chunks_per_segment {
            let (len, max) = (chunks.len() as u64 + 1, opts.max_chunks_per_segment as u64);
            return Err(StorageError::TooLarge {
                bound: Bound::SegmentChunks,
                len,
                max,
            });
        }
        at += h.meta.frame_len();
        chunks.push(h.meta);
    }
    Ok((chunks, at))
}
