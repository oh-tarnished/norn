//! Shared helpers: a scratch database, real segments, and a brute-force oracle.
#![allow(dead_code, clippy::unwrap_used, clippy::indexing_slicing)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use norn_core::{Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value};
use norn_index::{ChunkLocator, IndexOptions, PlanRequest, Snapshot, TemporalIndex};
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{
    Chunk, SegmentHeader, SegmentName, SegmentReader, SegmentSummary, SegmentWriter,
    StorageOptions, live_segments, stream_dir,
};

pub const STREAM: StreamId = StreamId::new(7);

/// A unique database root under the system temp dir, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("norn-index-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(stream_dir(&dir, STREAM)).unwrap();
        Self(dir)
    }

    pub fn root(&self) -> &Path {
        &self.0
    }

    pub fn dir(&self) -> PathBuf {
        stream_dir(&self.0, STREAM)
    }

    pub fn index_dir(&self) -> PathBuf {
        self.0.join("index").join(format!("{:016x}", STREAM.get()))
    }

    pub fn summary(&self) -> PathBuf {
        self.index_dir().join("summary.nidx")
    }

    pub fn sidecar(&self, seq: u64, generation: u16) -> PathBuf {
        self.index_dir()
            .join(format!("{seq:016x}-g{generation:04x}.ndet"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `n` samples of `signal`, `step` seconds apart, from second `start`.
pub fn chunk(signal: u64, start: i64, n: usize, step: i64) -> Chunk {
    let samples: Vec<_> = (0..n as i64)
        .map(|i| {
            let t = start + i * step;
            Sample::new(
                Timestamp::from_nanos(t * 1_000_000_000),
                SignalId::new(signal),
                Value::F64(t as f64),
                Quality::GOOD,
                Sequence::new(t as u64),
            )
        })
        .collect();
    Chunk::from_samples(&samples).unwrap()
}

pub fn secs(s: i64) -> Timestamp {
    Timestamp::from_nanos(s * 1_000_000_000)
}

/// A sealed segment of `chunks` at sequence `seq`.
pub fn sealed(dir: &Path, seq: u64, chunks: &[Chunk]) -> SegmentSummary {
    let mut w = writer(dir, seq);
    for c in chunks {
        w.append(c).unwrap();
    }
    w.seal().unwrap()
}

pub fn writer(dir: &Path, seq: u64) -> SegmentWriter {
    let header = SegmentHeader::new(STREAM, seq);
    SegmentWriter::create(dir, header, StorageOptions::default()).unwrap()
}

pub fn open(s: &Scratch) -> TemporalIndex {
    TemporalIndex::open(s.root(), IndexOptions::default())
        .unwrap()
        .0
}

/// Every locator a request yields, failing on an error.
pub fn plan(snapshot: &Arc<Snapshot>, request: PlanRequest) -> Vec<ChunkLocator> {
    snapshot.plan(request).map(Result::unwrap).collect()
}

/// `(segment, chunk)` pairs, for comparing answers across layouts of one state.
pub fn keys(locators: &[ChunkLocator]) -> Vec<(SegmentName, ChunkMeta)> {
    locators.iter().map(|l| (l.segment, l.chunk)).collect()
}

/// The oracle: every chunk of every live segment, read through the storage
/// layer's own directory and nothing of the index, in plan order.
pub fn brute(dir: &Path) -> Vec<ChunkLocator> {
    let mut all = Vec::new();
    for entry in live_segments(dir).unwrap() {
        let r = SegmentReader::open(&entry.path, StorageOptions::default()).unwrap();
        let sealed = r.is_sealed();
        let segment = SegmentName::of(r.header());
        all.extend(r.chunks().iter().map(|c| ChunkLocator {
            stream: STREAM,
            segment,
            sealed,
            chunk: *c,
        }));
    }
    all.sort();
    all
}
