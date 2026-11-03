//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The `CLEAN` file: all integers little-endian (docs/reference/storage.md).

use std::fs;
use std::path::Path;

use norn_core::{StreamId, Timestamp};

use super::CleanSegment;
use crate::crc32c::crc32c;
use crate::segment::SegmentName;

const MAGIC: &[u8; 8] = b"NORNCLN\0";
const VERSION: u16 = 1;
/// Magic, version, reserved, stream, count.
const HEAD: usize = 8 + 2 + 2 + 8 + 4;
/// Sequence, generation, reserved, bytes, samples, earliest, latest.
const ENTRY: usize = 8 + 2 + 2 + 8 + 8 + 8 + 8;
/// The longest marker read: over a million segments, a century at one an hour. A
/// longer file is not one this code wrote.
const MAX_FILE: u64 = 64 << 20;

/// The file's bytes, CRC included.
pub(super) fn encode(stream: StreamId, sorted: &[CleanSegment]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEAD + sorted.len() * ENTRY + 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&stream.get().to_le_bytes());
    let count = u32::try_from(sorted.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    for s in sorted {
        let (min, max) = s.time_range.unwrap_or((Timestamp::MAX, Timestamp::MIN));
        out.extend_from_slice(&s.name.seq.to_le_bytes());
        out.extend_from_slice(&s.name.generation.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&s.bytes.to_le_bytes());
        out.extend_from_slice(&s.samples.to_le_bytes());
        out.extend_from_slice(&min.as_nanos().to_le_bytes());
        out.extend_from_slice(&max.as_nanos().to_le_bytes());
    }
    out.extend_from_slice(&crc32c(&out).to_le_bytes());
    out
}

fn take<const N: usize>(bytes: &mut &[u8]) -> Option<[u8; N]> {
    let (head, rest) = bytes.split_first_chunk::<N>()?;
    *bytes = rest;
    Some(*head)
}

/// The entries of the file at `path`, or `None` if it is not a good marker for
/// `stream`. Copies.
pub(super) fn load(path: &Path, stream: StreamId) -> Option<Vec<CleanSegment>> {
    if fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (body, crc) = bytes.split_last_chunk::<4>()?;
    if crc32c(body) != u32::from_le_bytes(*crc) {
        return None;
    }
    let mut r = body;
    let right = take::<8>(&mut r)? == *MAGIC
        && u16::from_le_bytes(take(&mut r)?) == VERSION
        && take::<2>(&mut r).is_some()
        && u64::from_le_bytes(take(&mut r)?) == stream.get();
    let count = u32::from_le_bytes(take(&mut r)?) as usize;
    if !right || r.len() != count.checked_mul(ENTRY)? {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let seq = u64::from_le_bytes(take(&mut r)?);
        let generation = u16::from_le_bytes(take(&mut r)?);
        take::<2>(&mut r)?;
        let bytes = u64::from_le_bytes(take(&mut r)?);
        let samples = u64::from_le_bytes(take(&mut r)?);
        let min = Timestamp::from_nanos(i64::from_le_bytes(take(&mut r)?));
        let max = Timestamp::from_nanos(i64::from_le_bytes(take(&mut r)?));
        out.push(CleanSegment {
            name: SegmentName { seq, generation },
            bytes,
            samples,
            time_range: (min <= max).then_some((min, max)),
        });
    }
    Some(out)
}
