//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a sidecar's bytes. Everything is checked; `None` means damaged or for
//! another segment, and the caller repairs from the segment's directory.

use norn_core::{SignalId, StreamId, Timestamp, ValueKind};
use norn_storage::format::frame::ChunkMeta;

use super::{FENCE_LEN, FIXED_HEAD, MAGIC, PAGE_ENTRIES, VERSION};
use crate::record::SegmentRecord;
use crate::wire::{Cursor, checked_body, unzigzag};

/// Pages holding `chunks` chunks.
pub(crate) fn page_count(chunks: usize) -> usize {
    chunks.div_ceil(PAGE_ENTRIES)
}

/// Bytes of a head for `chunks` chunks: fixed part, fences, sentinel, CRC.
pub(crate) fn head_len(chunks: usize) -> usize {
    FIXED_HEAD + (page_count(chunks) + 1) * FENCE_LEN + 4
}

/// A sidecar's fence table, in memory.
///
/// Invariant: `offsets` has one entry more than `first_signals`, and never
/// decreases, so page `i` is the bytes `offsets[i]..offsets[i + 1]`.
///
/// Memory: 12 bytes a page, about 1.5 KiB for a compacted hour.
#[derive(Debug)]
pub(crate) struct DetailHead {
    first_signals: Box<[u64]>,
    offsets: Box<[u32]>,
}

impl DetailHead {
    /// Bytes this head holds.
    pub(crate) fn memory_bytes(&self) -> u64 {
        (self.first_signals.len() * 8 + self.offsets.len() * 4 + size_of::<Self>()) as u64
    }

    /// Number of pages.
    pub(crate) fn pages(&self) -> usize {
        self.first_signals.len()
    }

    /// The pages that can hold `signal`: chunks of one signal may span pages,
    /// and the page before the first one starting at `signal` may end with it.
    pub(crate) fn pages_of(&self, signal: SignalId) -> std::ops::Range<usize> {
        let s = signal.get();
        let first = self.first_signals.partition_point(|f| *f < s);
        let end = self.first_signals.partition_point(|f| *f <= s);
        first.saturating_sub(1)..end
    }

    /// The byte range of page `page` within the file.
    pub(crate) fn page_range(&self, page: usize) -> Option<(u64, usize)> {
        let start = *self.offsets.get(page)?;
        let end = *self.offsets.get(page + 1)?;
        let len = end.checked_sub(start)?;
        Some((u64::from(start), len as usize))
    }
}

/// Parses and verifies a head for `record`. Copies.
pub(crate) fn decode_head(
    bytes: &[u8],
    stream: StreamId,
    record: &SegmentRecord,
) -> Option<DetailHead> {
    let chunks = record.chunks as usize;
    let body = checked_body(bytes.get(..head_len(chunks))?)?;
    let mut r = Cursor::new(body);
    r.magic(MAGIC)?;
    (r.u16()? == VERSION).then_some(())?;
    let _reserved = r.u16()?;
    let pages = r.u32()? as usize;
    let same = pages == page_count(chunks)
        && r.u64()? == stream.get()
        && r.u64()? == record.first_seq
        && r.u16()? == record.generation
        && r.u16()? == 0
        && r.u32()? == record.chunks
        && r.u64()? == record.file_len;
    same.then_some(())?;
    let mut first_signals = Vec::with_capacity(pages);
    let mut offsets = Vec::with_capacity(pages + 1);
    for i in 0..=pages {
        let first = r.u64()?;
        let at = r.u32()?;
        if offsets.last().is_some_and(|prev| *prev > at) {
            return None;
        }
        offsets.push(at);
        if i < pages {
            first_signals.push(first);
        }
    }
    Some(DetailHead {
        first_signals: first_signals.into_boxed_slice(),
        offsets: offsets.into_boxed_slice(),
    })
}

/// Parses and verifies page `page` of a sidecar with `chunks` chunks. `bytes`
/// runs from the page's first byte through its CRC. Copies.
pub(crate) fn decode_page(bytes: &[u8], chunks: usize, page: usize) -> Option<Vec<ChunkMeta>> {
    let before = page.checked_mul(PAGE_ENTRIES)?;
    let count = chunks.checked_sub(before)?.min(PAGE_ENTRIES);
    let mut r = Cursor::new(checked_body(bytes)?);
    let mut out = Vec::with_capacity(count);
    let mut signal = 0u64;
    let (mut end, mut min) = (0u64, 0i64);
    for _ in 0..count {
        signal = signal.wrapping_add(r.varint()?);
        let offset = end.wrapping_add(unzigzag(r.varint()?) as u64);
        let body_len = u32::try_from(r.varint()?).ok()?;
        min = min.wrapping_add(unzigzag(r.varint()?));
        let max = min.wrapping_add(unzigzag(r.varint()?));
        let n = u32::try_from(r.varint()?).ok()?;
        let kind = ValueKind::from_tag(r.u8()?)?;
        let meta = ChunkMeta {
            offset,
            body_len,
            signal: SignalId::new(signal),
            kind,
            min_ts: Timestamp::from_nanos(min),
            max_ts: Timestamp::from_nanos(max),
            count: n,
        };
        end = offset.wrapping_add(meta.frame_len());
        out.push(meta);
    }
    r.rest().is_empty().then_some(out)
}
