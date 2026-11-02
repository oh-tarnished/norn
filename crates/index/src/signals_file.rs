//! `signals.nsig`: the per-signal summary of one stream, kept across restarts.
//!
//! docs/reference/storage.md has the byte table. The file is derived and tied to
//! the exact set of sealed segments it describes, by a fingerprint of their names
//! and lengths: anything else, and it is rebuilt from the sidecars.

use std::fs;
use std::path::Path;

use norn_core::{SignalId, StreamId, Timestamp};
use norn_storage::{SegmentName, atomic_write, crc32c};

use crate::record::SegmentRecord;
use crate::signals::{SignalMap, SignalSpan};
use crate::wire::{Cursor, checked_body};

/// The file's name, beside `summary.nidx`.
pub(crate) const SIGNALS_FILE: &str = "signals.nsig";
const MAGIC: &[u8; 8] = b"NORNSIG\0";
const VERSION: u16 = 1;
const ENTRY_LEN: usize = 34;

/// Identifies a set of sealed segments, in any order. Tails are left out: a tail
/// that survives a restart comes back as a sealed segment, which changes this.
pub(crate) fn fingerprint(records: &[SegmentRecord]) -> u32 {
    let mut parts: Vec<_> = records.iter().map(|r| (r.name(), r.file_len)).collect();
    parts.sort_unstable();
    let mut bytes = Vec::with_capacity(parts.len() * 18);
    for (name, len) in parts {
        bytes.extend_from_slice(&name.seq.to_le_bytes());
        bytes.extend_from_slice(&name.generation.to_le_bytes());
        bytes.extend_from_slice(&len.to_le_bytes());
    }
    crc32c(&bytes)
}

/// Encodes `map` for `stream` over segments of `print`.
pub(crate) fn encode(stream: StreamId, print: u32, map: &SignalMap) -> Vec<u8> {
    let mut b = Vec::with_capacity(32 + map.entries.len() * ENTRY_LEN);
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&VERSION.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&print.to_le_bytes());
    b.extend_from_slice(&stream.get().to_le_bytes());
    let count = u32::try_from(map.entries.len()).unwrap_or(u32::MAX);
    b.extend_from_slice(&count.to_le_bytes());
    for (signal, span) in &map.entries {
        b.extend_from_slice(&signal.get().to_le_bytes());
        b.extend_from_slice(&span.first.as_nanos().to_le_bytes());
        b.extend_from_slice(&span.last.as_nanos().to_le_bytes());
        b.extend_from_slice(&span.last_segment.seq.to_le_bytes());
        b.extend_from_slice(&span.last_segment.generation.to_le_bytes());
    }
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    b
}

/// Parses a map, copying. `None` for anything not exactly what `encode` wrote
/// for this stream and this set of segments: the caller rebuilds.
pub(crate) fn decode(bytes: &[u8], stream: StreamId, print: u32) -> Option<SignalMap> {
    let body = checked_body(bytes)?;
    let mut r = Cursor::new(body);
    r.magic(MAGIC)?;
    (r.u16()? == VERSION).then_some(())?;
    let _reserved = r.u16()?;
    (r.u32()? == print).then_some(())?;
    (r.u64()? == stream.get()).then_some(())?;
    let count = r.u32()? as usize;
    (r.rest().len() == count.checked_mul(ENTRY_LEN)?).then_some(())?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let signal = SignalId::new(r.u64()?);
        let first = Timestamp::from_nanos(r.i64()?);
        let last = Timestamp::from_nanos(r.i64()?);
        let seq = r.u64()?;
        let generation = r.u16()?;
        let last_segment = SegmentName { seq, generation };
        let span = SignalSpan {
            first,
            last,
            last_segment,
        };
        entries.push((signal, span));
    }
    entries
        .is_sorted_by_key(|e| e.0)
        .then_some(SignalMap { entries })
}

/// Loads the file at `path`, refusing one above `max_bytes` before reading it.
pub(crate) fn load(path: &Path, stream: StreamId, print: u32, max_bytes: u64) -> Option<SignalMap> {
    let len = fs::metadata(path).ok()?.len();
    let bytes = (len <= max_bytes).then(|| fs::read(path).ok()).flatten()?;
    decode(&bytes, stream, print)
}

/// Atomically writes the file at `path`.
pub(crate) fn save(
    path: &Path,
    stream: StreamId,
    print: u32,
    map: &SignalMap,
) -> norn_storage::Result<()> {
    atomic_write(path, &encode(stream, print, map))
}
