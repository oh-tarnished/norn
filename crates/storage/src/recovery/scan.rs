//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The recovery scan: verify every frame, and resynchronise past damage.

use crate::crypt::SegmentCipher;
use crate::error::{CorruptionKind, Result, StorageError};
use crate::format::frame::{self, ChunkMeta};
use crate::format::{DIR_MAGIC, FRAME_HEADER_LEN, FRAME_MAGIC};
use crate::fsio::Source;
use crate::options::StorageOptions;
use crate::segment::load::read_frame;

/// Bytes searched per read while looking for the next frame.
const WINDOW: usize = 64 * 1024;

/// One damaged region found by [`full_scan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Damage {
    pub(super) offset: u64,
    pub(super) len: u64,
    pub(super) kind: CorruptionKind,
}

/// What [`full_scan`] found between `start` and `end`.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// Frames whose header and body both verified, in file order.
    pub(super) chunks: Vec<ChunkMeta>,
    /// Damaged regions, in file order. A region reaching `end` is the tail.
    pub(super) damage: Vec<Damage>,
}

/// Verifies every frame from `start` to `end`.
///
/// A frame with a valid header but a bad body is damage of exactly its
/// recorded length (the header CRC vouches for the length), and the scan
/// continues after it. Bytes that are not a valid header are damage up to
/// the next valid frame header, found by searching for [`FRAME_MAGIC`]; if
/// none follows, the damage runs to `end` and is a torn tail, unless it
/// starts with [`DIR_MAGIC`], when it is a torn or corrupt directory.
///
/// Memory: one frame (at most `max_chunk_bytes`) and one search window.
///
/// # Errors
/// [`StorageError::Io`] only; damage is returned, not raised.
pub(super) fn full_scan(
    src: &mut Source,
    start: u64,
    end: u64,
    opts: &StorageOptions,
    cipher: Option<&SegmentCipher>,
) -> Result<Scan> {
    let mut scan = Scan::default();
    let mut at = start;
    let mut buf = Vec::new();
    while at < end {
        if let Some(meta) = header_at(src, at, end, opts)? {
            match read_frame(src, &meta, opts, cipher, &mut buf) {
                Ok(()) => scan.chunks.push(meta),
                Err(StorageError::Corrupt(c)) => {
                    scan.damage.push(Damage {
                        offset: at,
                        len: meta.frame_len(),
                        kind: c.kind,
                    });
                }
                Err(e) => return Err(e),
            }
            at += meta.frame_len();
            continue;
        }
        let is_dir = starts_with(src, at, end, &DIR_MAGIC)?;
        let next = if is_dir {
            None
        } else {
            next_frame(src, at + 1, end, opts)?
        };
        let stop = next.unwrap_or(end);
        let kind = match (next, is_dir) {
            (_, true) => CorruptionKind::Directory,
            (None, false) => CorruptionKind::TornTail,
            (Some(_), false) => CorruptionKind::FrameHeader,
        };
        scan.damage.push(Damage {
            offset: at,
            len: stop - at,
            kind,
        });
        at = stop;
    }
    Ok(scan)
}

/// The frame whose valid header starts exactly at `at` and fits before
/// `end`, if any.
fn header_at(
    src: &mut Source,
    at: u64,
    end: u64,
    opts: &StorageOptions,
) -> Result<Option<ChunkMeta>> {
    if end.saturating_sub(at) < FRAME_HEADER_LEN as u64 {
        return Ok(None);
    }
    let mut h = [0u8; FRAME_HEADER_LEN];
    src.read_at(at, &mut h)?;
    Ok(frame::decode(&h, at)
        .map(|h| h.meta)
        .filter(|m| m.body_len <= opts.max_chunk_bytes && at.saturating_add(m.frame_len()) <= end))
}

fn starts_with(src: &mut Source, at: u64, end: u64, magic: &[u8]) -> Result<bool> {
    if end.saturating_sub(at) < magic.len() as u64 {
        return Ok(false);
    }
    let mut b = vec![0u8; magic.len()];
    src.read_at(at, &mut b)?;
    Ok(b == magic)
}

/// The offset of the first valid frame header at or after `from`.
fn next_frame(src: &mut Source, from: u64, end: u64, opts: &StorageOptions) -> Result<Option<u64>> {
    let mut base = from;
    let mut window = Vec::new();
    while base < end {
        let n = usize::try_from((end - base).min(WINDOW as u64)).unwrap_or(WINDOW);
        src.read_vec(base, n, &mut window)?;
        let hits: Vec<u64> = window
            .windows(FRAME_MAGIC.len())
            .enumerate()
            .filter(|(_, w)| *w == FRAME_MAGIC)
            .map(|(i, _)| base + i as u64)
            .collect();
        for at in hits {
            if header_at(src, at, end, opts)?.is_some() {
                return Ok(Some(at));
            }
        }
        // Overlap by three bytes so a magic split across windows is found.
        let step = (n as u64)
            .saturating_sub(FRAME_MAGIC.len() as u64 - 1)
            .max(1);
        base += step;
    }
    Ok(None)
}
