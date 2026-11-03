//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared helpers: a scratch directory and sample builders.
#![allow(dead_code, clippy::unwrap_used, clippy::indexing_slicing)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use norn_core::{
    Quality, Sample, Sequence, Severity, SignalId, StreamId, Substatus, Timestamp, Value,
};
use norn_storage::{Chunk, SegmentHeader, SegmentSummary, SegmentWriter, StorageOptions};

/// A unique directory under the system temp dir, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("norn-storage-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub const STREAM: StreamId = StreamId::new(7);

/// `n` F64 samples of `signal`, one a second from second `start`.
pub fn samples(signal: u64, start: i64, n: usize) -> Vec<Sample> {
    (0..n)
        .map(|i| {
            let t = start + i as i64;
            let quality = if i % 50 == 49 {
                Quality::new(Severity::Uncertain, Substatus::Stale)
            } else {
                Quality::GOOD
            };
            Sample::new(
                Timestamp::from_nanos(t * 1_000_000_000),
                SignalId::new(signal),
                Value::F64(20.0 + (t % 100) as f64 / 10.0),
                quality,
                Sequence::new(t as u64),
            )
        })
        .collect()
}

pub fn chunk(signal: u64, start: i64, n: usize) -> Chunk {
    Chunk::from_samples(&samples(signal, start, n)).unwrap()
}

/// Every sample's bit-exact byte form, for comparing NaN-bearing data.
pub fn bytes_of(samples: &[Sample]) -> Vec<(i64, u64, Vec<u8>, u16, u64)> {
    samples
        .iter()
        .map(|s| {
            let mut v = Vec::new();
            s.value.encode(&mut v).unwrap();
            (
                s.timestamp.as_nanos(),
                s.signal.get(),
                v,
                s.quality.code(),
                s.sequence.get(),
            )
        })
        .collect()
}

/// A segment of `chunks` at sequence `seq`, left unsealed (as after a crash
/// between the last sync and the seal). Returns its path.
pub fn unsealed(dir: &Path, seq: u64, chunks: &[Chunk]) -> PathBuf {
    let mut w = SegmentWriter::create(
        dir,
        SegmentHeader::new(STREAM, seq),
        StorageOptions::default(),
    )
    .unwrap();
    for c in chunks {
        w.append(c).unwrap();
    }
    w.sync().unwrap();
    w.path().to_path_buf()
}

/// A sealed segment of `chunks` at sequence `seq`.
pub fn sealed(dir: &Path, seq: u64, chunks: &[Chunk]) -> SegmentSummary {
    let mut w = SegmentWriter::create(
        dir,
        SegmentHeader::new(STREAM, seq),
        StorageOptions::default(),
    )
    .unwrap();
    for c in chunks {
        w.append(c).unwrap();
    }
    w.seal().unwrap()
}

/// Every sample in a segment, in chunk order.
pub fn read_all(path: &Path) -> Vec<Sample> {
    let mut r = norn_storage::SegmentReader::open(path, StorageOptions::default()).unwrap();
    r.samples(None, Timestamp::MIN, Timestamp::MAX)
        .map(Result::unwrap)
        .collect()
}

/// Two segments of many small chunks over three signals.
pub fn small_chunks(dir: &Path) -> (Vec<PathBuf>, Vec<Sample>) {
    let (mut paths, mut all) = (Vec::new(), Vec::new());
    for seq in 0..2u64 {
        let mut chunks = Vec::new();
        for slice in 0..10i64 {
            for signal in 1..=3u64 {
                let start = seq as i64 * 1000 + slice * 10;
                chunks.push(chunk(signal, start, 10));
            }
        }
        all.extend(chunks.iter().flat_map(Chunk::samples));
        paths.push(sealed(dir, seq, &chunks).path);
    }
    (paths, all)
}
