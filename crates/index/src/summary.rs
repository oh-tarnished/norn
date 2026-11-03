//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `summary.nidx`: the persisted segment list of one stream.
//!
//! docs/reference/storage.md has the byte table. Written with `norn_storage::atomic_write`, so
//! a crash leaves the old file or the new one; every byte is under one CRC.

use std::fs;
use std::path::Path;

use norn_core::StreamId;
use norn_storage::{atomic_write, crc32c};

use crate::record::{RECORD_LEN, SegmentRecord};
use crate::report::{DiscardReason, SummaryState};
use crate::wire::{Cursor, checked_body};

/// The summary's file name.
pub(crate) const SUMMARY_FILE: &str = "summary.nidx";
const MAGIC: &[u8; 8] = b"NORNIDX\0";
const VERSION: u16 = 1;
const HEAD_LEN: usize = 24;

/// Encodes `records` for `stream`, in the order given.
pub(crate) fn encode(stream: StreamId, records: &[SegmentRecord]) -> Vec<u8> {
    let mut b = Vec::with_capacity(HEAD_LEN + records.len() * RECORD_LEN + 4);
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&VERSION.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(
        &u32::try_from(records.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    b.extend_from_slice(&stream.get().to_le_bytes());
    for r in records {
        r.encode(&mut b);
    }
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    b
}

/// Parses a summary; copies into the returned vector.
///
/// The magic and version are checked before the CRC, so a newer file is
/// reported as newer even though its layout, CRC included, may differ.
pub(crate) fn decode(bytes: &[u8], stream: StreamId) -> Result<Vec<SegmentRecord>, DiscardReason> {
    let mut head = Cursor::new(bytes);
    if head.rest().len() < MAGIC.len() {
        return Err(DiscardReason::Malformed);
    }
    head.magic(MAGIC).ok_or(DiscardReason::BadMagic)?;
    let version = head.u16().ok_or(DiscardReason::Malformed)?;
    if version > VERSION || version == 0 {
        return Err(DiscardReason::NewerVersion { found: version });
    }
    let body = checked_body(bytes).ok_or(DiscardReason::BadChecksum)?;
    let mut r = Cursor::new(body);
    r.magic(MAGIC).ok_or(DiscardReason::BadMagic)?;
    let _version = r.u16();
    let _reserved = r.u16();
    let count = r.u32().ok_or(DiscardReason::Malformed)? as usize;
    if r.u64().ok_or(DiscardReason::Malformed)? != stream.get() {
        return Err(DiscardReason::WrongStream);
    }
    if r.rest().len() != count.saturating_mul(RECORD_LEN) {
        return Err(DiscardReason::Malformed);
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(SegmentRecord::decode(&mut r).ok_or(DiscardReason::Malformed)?);
    }
    Ok(out)
}

/// Loads the summary at `path`, refusing one larger than `max_bytes` before
/// reading it. Any failure is a state to report, never an error.
pub(crate) fn load(
    path: &Path,
    stream: StreamId,
    max_bytes: u64,
) -> (Vec<SegmentRecord>, SummaryState) {
    let discard = |reason| (Vec::new(), SummaryState::Discarded(reason));
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), SummaryState::Missing);
        }
        Err(_) => return discard(DiscardReason::Unreadable),
    };
    if len > max_bytes {
        return discard(DiscardReason::Malformed);
    }
    let Ok(bytes) = fs::read(path) else {
        return discard(DiscardReason::Unreadable);
    };
    match decode(&bytes, stream) {
        Ok(records) => (records, SummaryState::Loaded),
        Err(reason) => discard(reason),
    }
}

/// Atomically writes the summary at `path`.
pub(crate) fn save(
    path: &Path,
    stream: StreamId,
    records: &[SegmentRecord],
) -> norn_storage::Result<()> {
    atomic_write(path, &encode(stream, records))
}
