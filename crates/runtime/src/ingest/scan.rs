//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading stored chunks back at startup: where each signal got to, and which
//! journal samples already made it into a segment.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use norn_core::{Sample, SignalId};
use norn_metadata::Registry;
use norn_storage::{SegmentReader, StorageError, StorageOptions, live_segments};

use super::dedup::Dedup;

/// A sample's identity.
pub(crate) type Key = (u64, i64, u64);

/// The identity of `s`.
pub(crate) fn key(s: &Sample) -> Key {
    (s.signal.get(), s.timestamp.as_nanos(), s.sequence.get())
}

/// Finds what the stored segments say about `fresh`, the journal's samples: any
/// key found is removed, since that sample needs no second write. Along the way
/// it feeds `dedup` the newest chunks of each signal and marks every signal seen
/// as holding data.
///
/// Reads every directory once, then only the chunks that can matter. A chunk
/// that fails to read was reported by recovery already; it is skipped, so a
/// journal sample inside it is written again, never lost.
pub(crate) fn reconcile(
    dir: &Path,
    opts: &StorageOptions,
    fresh: &mut HashSet<Key>,
    registry: &Registry,
    dedup: &mut Dedup,
) -> Result<(), StorageError> {
    let mut spans: HashMap<u64, (i64, i64)> = HashMap::new();
    for key in fresh.iter() {
        let span = spans.entry(key.0).or_insert((key.1, key.1));
        *span = (span.0.min(key.1), span.1.max(key.1));
    }
    let segments = live_segments(dir)?;
    let mut newest: HashMap<SignalId, i64> = HashMap::new();
    for segment in &segments {
        let reader = SegmentReader::open(&segment.path, *opts)?;
        for meta in reader.chunks() {
            let max = meta.max_ts.as_nanos();
            let best = newest.entry(meta.signal).or_insert(max);
            *best = (*best).max(max);
        }
    }
    for signal in newest.keys() {
        registry.mark_stored(*signal);
    }
    for segment in &segments {
        let mut reader = SegmentReader::open(&segment.path, *opts)?;
        let chunks = reader.chunks().to_vec();
        for meta in chunks {
            let is_newest = newest.get(&meta.signal) == Some(&meta.max_ts.as_nanos());
            let overlaps = spans
                .get(&meta.signal.get())
                .is_some_and(|(a, b)| meta.min_ts.as_nanos() <= *b && *a <= meta.max_ts.as_nanos());
            if !is_newest && !overlaps {
                continue;
            }
            let Ok(chunk) = reader.read_chunk(&meta) else {
                continue;
            };
            for sample in chunk.samples() {
                if overlaps {
                    fresh.remove(&key(&sample));
                }
                dedup.admit(&sample);
            }
        }
    }
    Ok(())
}
