//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing a sidecar's bytes.

use norn_core::StreamId;
use norn_storage::crc32c;
use norn_storage::format::frame::ChunkMeta;

use super::{FENCE_LEN, FIXED_HEAD, MAGIC, PAGE_ENTRIES, VERSION, page_count};
use crate::record::SegmentRecord;
use crate::wire::{put_varint, zigzag};

/// Encodes the sidecar of the segment `record` describes. Sorts `chunks` in
/// place by `(signal, min_ts, offset)`.
///
/// Cost: O(n log n) over the segment's chunks. Memory: the output, about 18
/// bytes a chunk.
pub(crate) fn encode_detail(
    stream: StreamId,
    record: &SegmentRecord,
    chunks: &mut [ChunkMeta],
) -> Vec<u8> {
    chunks.sort_by_key(|c| (c.signal, c.min_ts, c.offset));
    let pages = page_count(chunks.len());
    let fences_at = FIXED_HEAD;
    let body_at = fences_at + (pages + 1) * FENCE_LEN + 4;
    let mut body = Vec::with_capacity(chunks.len() * 20);
    let mut fences = Vec::with_capacity((pages + 1) * FENCE_LEN);
    for page in chunks.chunks(PAGE_ENTRIES) {
        let first = page.first().map_or(0, |c| c.signal.get());
        fences.extend_from_slice(&first.to_le_bytes());
        let at = u32::try_from(body_at + body.len()).unwrap_or(u32::MAX);
        fences.extend_from_slice(&at.to_le_bytes());
        let start = body.len();
        encode_page(page, &mut body);
        let crc = crc32c(body.get(start..).unwrap_or_default());
        body.extend_from_slice(&crc.to_le_bytes());
    }
    fences.extend_from_slice(&u64::MAX.to_le_bytes());
    let end = u32::try_from(body_at + body.len()).unwrap_or(u32::MAX);
    fences.extend_from_slice(&end.to_le_bytes());

    let mut out = Vec::with_capacity(body_at + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&u32::try_from(pages).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&stream.get().to_le_bytes());
    out.extend_from_slice(&record.first_seq.to_le_bytes());
    out.extend_from_slice(&record.generation.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(chunks.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(&record.file_len.to_le_bytes());
    out.extend_from_slice(&fences);
    let crc = crc32c(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// Appends one page's entries; `decode_page` is the inverse. Deltas start from
/// zero, so a page decodes alone. Every delta wraps, so any value round-trips,
/// however odd.
fn encode_page(page: &[ChunkMeta], out: &mut Vec<u8>) {
    let mut signal = 0u64;
    let (mut end, mut min) = (0u64, 0i64);
    for c in page {
        put_varint(out, c.signal.get().wrapping_sub(signal));
        signal = c.signal.get();
        put_varint(out, zigzag(c.offset.wrapping_sub(end) as i64));
        end = c.offset.wrapping_add(c.frame_len());
        put_varint(out, u64::from(c.body_len));
        put_varint(out, zigzag(c.min_ts.as_nanos().wrapping_sub(min)));
        min = c.min_ts.as_nanos();
        put_varint(out, zigzag(c.max_ts.as_nanos().wrapping_sub(min)));
        put_varint(out, u64::from(c.count));
        out.push(c.kind.tag());
    }
}
