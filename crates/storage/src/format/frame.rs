//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The chunk frame header and the chunk metadata it carries.
//!
//! | Offset | Size | Field |
//! | ---: | ---: | --- |
//! | 0 | 4 | magic `NCHK`, the recovery sync mark |
//! | 4 | 1 | frame version, 1 |
//! | 5 | 1 | value kind (`norn_core::ValueKind::tag`) |
//! | 6 | 2 | flags, zero (reserved for multi-signal groups) |
//! | 8 | 8 | signal id |
//! | 16 | 8 | earliest timestamp (ns) |
//! | 24 | 8 | latest timestamp (ns) |
//! | 32 | 4 | sample count |
//! | 36 | 4 | body length |
//! | 40 | 4 | CRC-32C of the body |
//! | 44 | 4 | CRC-32C of bytes 0..44 |
//!
//! The header CRC lets a scanner trust the body length before reading the
//! body, so it can skip a chunk it does not want and detect a frame torn
//! anywhere: inside the header (CRC), or inside the body (length, then CRC).

use norn_core::{SignalId, Timestamp, ValueKind};

use super::{FRAME_HEADER_LEN, FRAME_MAGIC, Le};
use crate::crc32c::crc32c;

/// The frame layout version written in byte 4.
const FRAME_VERSION: u8 = 1;

/// Where a chunk is and what it holds, without decoding it.
///
/// This is what a segment's directory lists and what partial reads select
/// on: signal and time range are enough to skip a chunk unread.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkMeta {
    /// Byte offset of the frame header within the segment file.
    pub offset: u64,
    /// Bytes of body after the frame header.
    pub body_len: u32,
    /// The signal whose samples the chunk holds.
    pub signal: SignalId,
    /// The kind of every value in the chunk.
    pub kind: ValueKind,
    /// The earliest sample timestamp.
    pub min_ts: Timestamp,
    /// The latest sample timestamp.
    pub max_ts: Timestamp,
    /// How many samples the chunk holds.
    pub count: u32,
}

impl ChunkMeta {
    /// Total bytes of the frame on disk: header plus body.
    #[must_use]
    pub fn frame_len(&self) -> u64 {
        FRAME_HEADER_LEN as u64 + u64::from(self.body_len)
    }

    /// Whether any sample of the chunk may fall in `from..=to`.
    #[must_use]
    pub fn overlaps(&self, from: Timestamp, to: Timestamp) -> bool {
        self.min_ts <= to && self.max_ts >= from
    }
}

/// A frame header as read: the metadata plus the body's recorded CRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameHeader {
    pub(crate) meta: ChunkMeta,
    pub(crate) body_crc: u32,
}

/// Encodes a frame header for `meta` and a body with CRC `body_crc`.
pub(crate) fn encode(meta: &ChunkMeta, body_crc: u32) -> [u8; FRAME_HEADER_LEN] {
    let mut b = Vec::with_capacity(FRAME_HEADER_LEN);
    b.extend_from_slice(&FRAME_MAGIC);
    b.extend([FRAME_VERSION, meta.kind.tag()]);
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&meta.signal.get().to_le_bytes());
    b.extend_from_slice(&meta.min_ts.as_nanos().to_le_bytes());
    b.extend_from_slice(&meta.max_ts.as_nanos().to_le_bytes());
    b.extend_from_slice(&meta.count.to_le_bytes());
    b.extend_from_slice(&meta.body_len.to_le_bytes());
    b.extend_from_slice(&body_crc.to_le_bytes());
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    let mut out = [0u8; FRAME_HEADER_LEN];
    out.copy_from_slice(&b);
    out
}

/// Parses a frame header found at file offset `offset`. Returns `None` when
/// the bytes are not a valid header: wrong magic or length, bad CRC, an
/// unknown frame version or value kind. Copies; borrows nothing past the
/// call.
pub(crate) fn decode(bytes: &[u8], offset: u64) -> Option<FrameHeader> {
    let rec = bytes.get(..FRAME_HEADER_LEN)?;
    let (body, crc) = rec.split_at(FRAME_HEADER_LEN - 4);
    let mut r = Le::new(rec);
    if r.bytes::<4>()? != FRAME_MAGIC || crc != crc32c(body).to_le_bytes() {
        return None;
    }
    if r.u8()? != FRAME_VERSION {
        return None;
    }
    let kind = ValueKind::from_tag(r.u8()?)?;
    let _flags = r.u16()?;
    let signal = SignalId::new(r.u64()?);
    let min_ts = Timestamp::from_nanos(r.i64()?);
    let max_ts = Timestamp::from_nanos(r.i64()?);
    let count = r.u32()?;
    let body_len = r.u32()?;
    let body_crc = r.u32()?;
    let meta = ChunkMeta {
        offset,
        body_len,
        signal,
        kind,
        min_ts,
        max_ts,
        count,
    };
    Some(FrameHeader { meta, body_crc })
}
