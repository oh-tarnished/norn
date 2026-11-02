//! Reading a log back: what is a record, what is a torn tail, what is damage.

use std::path::Path;

use super::{HEADER_LEN, MAGIC, MAX_RECORD_BYTES, RECORD_HEADER_LEN, VERSION};
use crate::crc::crc32c;
use crate::error::MetadataError;
use crate::op::Op;

/// The log's fixed header bytes.
pub(super) fn header() -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&VERSION.to_le_bytes());
    let crc = crc32c(&h);
    h.extend_from_slice(&crc.to_le_bytes());
    h
}

/// Walks `bytes` (a whole log), applying each record, and returns the offset
/// where good records end. Borrows `bytes`; each [`Op`] owns its contents.
///
/// A record cut short by the end of the file, or whose checksum fails with
/// nothing after it, is a torn write: returned as the end, for the caller to
/// cut. A bad record with more log after it is damage, because the records
/// behind it were acknowledged and skipping would silently undo them.
pub(super) fn scan(
    bytes: &[u8],
    path: &Path,
    mut apply: impl FnMut(Op),
) -> Result<(usize, u64), MetadataError> {
    let corrupt = |offset: usize, what| MetadataError::Corrupt {
        path: path.to_path_buf(),
        offset: offset as u64,
        what,
    };
    let head = bytes
        .get(..HEADER_LEN)
        .ok_or_else(|| corrupt(0, "header"))?;
    if head != header() {
        return Err(bad_header(head).unwrap_or_else(|| corrupt(0, "header")));
    }
    let (mut at, mut records) = (HEADER_LEN, 0u64);
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
        apply(Op::decode(payload).ok_or_else(|| corrupt(at, "record contents"))?);
        records += 1;
        at = end;
    }
    Ok((at, records))
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
