//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A scratch database, a fake tail, a counting budget and a brute-force oracle.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use norn_core::{Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value, ValueKind};
use norn_query::Target;
use norn_storage::{Chunk, SegmentSummary, StorageOptions, StreamWriter, stream_dir};

pub const STREAM: StreamId = StreamId::new(7);

/// A unique database root, removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("norn-query-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(stream_dir(&dir, STREAM)).unwrap();
        Self(dir)
    }

    pub fn dir(&self) -> PathBuf {
        stream_dir(&self.0, STREAM)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn at(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}

pub fn sample(signal: u64, t: i64, value: Value) -> Sample {
    let (ts, id) = (at(t), SignalId::new(signal));
    Sample::new(ts, id, value, Quality::GOOD, Sequence::new(t as u64))
}

pub fn target(signal: u64, kind: ValueKind) -> Target {
    Target {
        name: format!("entities/e/signals/s{signal:03}").into(),
        signal: SignalId::new(signal),
        stream: STREAM,
        kind,
    }
}

/// Writes one sealed segment per entry of `segments`, one chunk per signal and
/// per `chunk` samples, in the order given.
pub fn store(dir: &Path, segments: &[Vec<Sample>], chunk: usize) -> Vec<SegmentSummary> {
    let mut writer = StreamWriter::open(dir, STREAM, StorageOptions::default()).unwrap();
    let mut sealed = Vec::new();
    for segment in segments {
        let mut by: std::collections::BTreeMap<u64, Vec<Sample>> = Default::default();
        for s in segment {
            by.entry(s.signal.get()).or_default().push(s.clone());
        }
        for samples in by.values() {
            for part in samples.chunks(chunk.max(1)) {
                writer.append(&Chunk::from_samples(part).unwrap()).unwrap();
            }
        }
        sealed.extend(writer.roll().unwrap());
    }
    writer.close().unwrap();
    sealed
}

mod fakes;
pub mod oracle;
pub use fakes::*;
