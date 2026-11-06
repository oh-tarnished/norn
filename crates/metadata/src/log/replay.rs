//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a log back: what is a record, what is a torn tail, what is damage.

use std::path::Path;

use super::{HEADER_LEN, MAGIC, MAX_RECORD_BYTES, RECORD_HEADER_LEN, VERSION, VERSION_1};
use crate::crc::crc32c;
use crate::error::MetadataError;
use crate::op::Op;

/// The log's fixed header bytes, for the current version.
pub(crate) fn header() -> Vec<u8> {
    header_of(VERSION)
}

fn header_of(version: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&version.to_le_bytes());
    let crc = crc32c(&h);
    h.extend_from_slice(&crc.to_le_bytes());
    h
}

/// Walks `bytes` (a whole log), applying each record, and returns the offset
/// where good records end, how many there were, and the header's version.
/// Borrows `bytes`; each [`Op`] owns its contents.
///
/// A record cut short by the end of the file, or whose checksum fails with
/// nothing after it, is a torn write: returned as the end, for the caller to
/// cut. A bad record with more log after it is damage, because the records
/// behind it were acknowledged and skipping would silently undo them.
///
/// A batch's records are held until its last is read and then applied
/// together. A log ending inside a batch ends at the batch's marker, so the
/// whole unacknowledged batch is the torn tail. Holding them costs at most the
/// log already in memory.
pub(crate) fn scan(
    bytes: &[u8],
    path: &Path,
    mut apply: impl FnMut(Op),
) -> Result<(usize, u64, u32), MetadataError> {
    let corrupt = |offset: usize, what| MetadataError::Corrupt {
        path: path.to_path_buf(),
        offset: offset as u64,
        what,
    };
    let head = bytes
        .get(..HEADER_LEN)
        .ok_or_else(|| corrupt(0, "header"))?;
    let version = if head == header() {
        VERSION
    } else if head == header_of(VERSION_1) {
        VERSION_1
    } else {
        return Err(bad_header(head).unwrap_or_else(|| corrupt(0, "header")));
    };
    let (mut at, mut records) = (HEADER_LEN, 0u64);
    // An open batch: where its marker starts, the record count before it, how
    // many records are still owed, and those read so far.
    let mut batch: Option<(usize, u64, u64, Vec<Op>)> = None;
    while let Some(head) = bytes.get(at..at + RECORD_HEADER_LEN) {
        let (len, crc) = (word(head, 0), word(head, 4));
        let start = at + RECORD_HEADER_LEN;
        let end = start.saturating_add(len as usize);
        if len > MAX_RECORD_BYTES {
            if end < bytes.len() {
                return Err(corrupt(at, "record length"));
            }
            break;
        }
        let Some(payload) = bytes.get(start..end) else {
            break;
        };
        if crc32c(payload) != crc {
            if end == bytes.len() {
                break;
            }
            return Err(corrupt(at, "record checksum"));
        }
        let op = Op::decode(payload).ok_or_else(|| corrupt(at, "record contents"))?;
        match (op, batch.as_mut()) {
            (Op::Begin(_), Some(_)) => return Err(corrupt(at, "nested batch")),
            (Op::Begin(0), None) => {}
            (Op::Begin(n), None) => batch = Some((at, records, n, Vec::new())),
            (op, None) => apply(op),
            (op, Some((_, _, owed, held))) => {
                held.push(op);
                *owed = owed.saturating_sub(1);
                if *owed == 0 {
                    batch
                        .take()
                        .into_iter()
                        .flat_map(|b| b.3)
                        .for_each(&mut apply);
                }
            }
        }
        records += 1;
        at = end;
    }
    match batch {
        Some((start, before, ..)) => Ok((start, before, version)),
        None => Ok((at, records, version)),
    }
}

/// The little-endian `u32` at `offset` of `bytes`, or zero past the end.
fn word(bytes: &[u8], offset: usize) -> u32 {
    bytes
        .get(offset..offset + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_le_bytes)
}

/// An intact header of another version, if `head` is one.
fn bad_header(head: &[u8]) -> Option<MetadataError> {
    let intact = head.get(..8) == Some(MAGIC.as_slice())
        && word(head, 12) == crc32c(head.get(..12).unwrap_or_default());
    intact.then(|| MetadataError::UnsupportedVersion {
        found: word(head, 8),
    })
}
