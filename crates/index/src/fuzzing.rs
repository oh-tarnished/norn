//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The index's file decoders, exposed for the fuzz crate only (feature
//! `fuzzing`). Each is the function the index itself calls on bytes read from
//! disk, with the encoder that is its inverse, so a fuzz target can check both
//! "never panics" and "decode is the inverse of encode". Not an API: no
//! shipped build enables the feature, and these may change without notice.

use norn_core::{SignalId, StreamId, Timestamp};
use norn_storage::SegmentName;
use norn_storage::format::frame::ChunkMeta;

use crate::detail::{decode_head, decode_page, encode_detail, head_len};
use crate::record::{RECORD_LEN, SegmentRecord};
use crate::report::DiscardReason;
use crate::signals::{SignalMap, SignalSpan};
use crate::tail::TailLog;
use crate::wire::Cursor;
use crate::{signals_file, summary};

/// Bytes of one `summary.nidx` record.
pub const SUMMARY_RECORD_LEN: usize = RECORD_LEN;

/// Parses `summary.nidx` bytes for `stream`, as startup does. Copies.
///
/// # Errors
/// The [`DiscardReason`] startup would report.
pub fn decode_summary(bytes: &[u8], stream: StreamId) -> Result<Vec<SegmentRecord>, DiscardReason> {
    summary::decode(bytes, stream)
}

/// Encodes a `summary.nidx` for `stream`.
#[must_use]
pub fn encode_summary(stream: StreamId, records: &[SegmentRecord]) -> Vec<u8> {
    summary::encode(stream, records)
}

/// Parses one 72-byte summary record from the front of `bytes`. Copies.
#[must_use]
pub fn decode_record(bytes: &[u8]) -> Option<SegmentRecord> {
    SegmentRecord::decode(&mut Cursor::new(bytes))
}

/// Parses `signals.nsig` bytes for `stream` over segments of fingerprint
/// `print`, as startup does. Copies.
#[must_use]
pub fn decode_signals(
    bytes: &[u8],
    stream: StreamId,
    print: u32,
) -> Option<Vec<(SignalId, SignalSpan)>> {
    signals_file::decode(bytes, stream, print).map(|m| m.entries)
}

/// Encodes a `signals.nsig`; `entries` must be sorted by signal, as the
/// index keeps them.
#[must_use]
pub fn encode_signals(
    stream: StreamId,
    print: u32,
    entries: Vec<(SignalId, SignalSpan)>,
) -> Vec<u8> {
    signals_file::encode(stream, print, &SignalMap { entries })
}

/// Parses a whole `*.ndet` sidecar for `record`: its head, then every page
/// the head lists, as lookups read them one at a time. Copies.
#[must_use]
pub fn decode_detail(
    bytes: &[u8],
    stream: StreamId,
    record: &SegmentRecord,
) -> Option<Vec<ChunkMeta>> {
    let head = decode_head(bytes, stream, record)?;
    let mut out = Vec::new();
    for page in 0..head.pages() {
        let (at, len) = head.page_range(page)?;
        let at = usize::try_from(at).ok()?;
        let page_bytes = bytes.get(at..at.checked_add(len)?)?;
        out.extend(decode_page(page_bytes, record.chunks as usize, page)?);
    }
    Some(out)
}

/// Encodes the sidecar of `record`, sorting `chunks` as the index does.
#[must_use]
pub fn encode_detail_file(
    stream: StreamId,
    record: &SegmentRecord,
    chunks: &mut [ChunkMeta],
) -> Vec<u8> {
    encode_detail(stream, record, chunks)
}

/// Bytes of a sidecar's head for `chunks` chunks, CRC included.
#[must_use]
pub fn detail_head_len(chunks: u32) -> usize {
    head_len(chunks as usize)
}

/// Parses one sidecar page. `bytes` runs through its CRC. Copies.
#[must_use]
pub fn decode_detail_page(bytes: &[u8], chunks: u32, page: usize) -> Option<Vec<ChunkMeta>> {
    decode_page(bytes, chunks as usize, page)
}

/// Publishes `batches` into a tail log in turn, as the commit path does, and
/// reports the chunks it holds, what `for_each` calls back with over
/// `from..=to`, its range and its accounted bytes.
#[must_use]
pub fn tail_log(
    batches: &[Vec<ChunkMeta>],
    from: Timestamp,
    to: Timestamp,
) -> (usize, Vec<ChunkMeta>, Option<(Timestamp, Timestamp)>, u64) {
    let name = SegmentName {
        seq: 0,
        generation: 0,
    };
    let log = batches
        .iter()
        .fold(TailLog::new(name), |log, batch| log.appended(batch));
    let mut seen = Vec::new();
    log.for_each(from, to, |m| seen.push(*m));
    (log.len(), seen, log.range(), log.memory_bytes())
}
