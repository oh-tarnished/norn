//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What encryption at rest costs: the same chunks written and read plain and
//! encrypted, through the real writer and reader, best of three runs.
//!
//! `cargo bench -p norn-storage --bench encryption` prints a Markdown table.
//! Single-threaded and file-cache warm: it measures the cipher's share of the
//! CPU, not the disk. `cargo test --all-targets` runs a tiny smoke scale;
//! `NORN_BENCH_CHUNKS` overrides the scale.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use norn_core::{Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value};
use norn_storage::{Chunk, Keyring, SegmentHeader, SegmentReader, SegmentWriter, StorageOptions};

const KEY: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
const SAMPLES: usize = 8_640;

fn chunk(signal: u64) -> Chunk {
    let samples: Vec<Sample> = (0..SAMPLES)
        .map(|i| {
            let at = Timestamp::from_nanos(i as i64 * 10_000_000_000);
            let value = Value::F64((i as f64 * 0.01).sin() * 100.0 + signal as f64);
            Sample::new(
                at,
                SignalId::new(signal),
                value,
                Quality::GOOD,
                Sequence::ZERO,
            )
        })
        .collect();
    Chunk::from_samples(&samples).unwrap()
}

/// Best (write, read) times of three, and the file's size.
fn run(dir: &Path, chunks: &[Chunk], opts: StorageOptions) -> (Duration, Duration, u64) {
    let (mut write, mut read, mut size) = (Duration::MAX, Duration::MAX, 0);
    for round in 0..3u64 {
        let started = Instant::now();
        let header = SegmentHeader::new(StreamId::new(1), round);
        let mut w = SegmentWriter::create(dir, header, opts).unwrap();
        for c in chunks {
            w.append(c).unwrap();
        }
        let path = w.seal().unwrap().path;
        write = write.min(started.elapsed());
        let started = Instant::now();
        let mut r = SegmentReader::open(&path, opts).unwrap();
        let metas = r.chunks().to_vec();
        let n: usize = metas.iter().map(|m| r.read_chunk(m).unwrap().len()).sum();
        assert_eq!(n, chunks.len() * SAMPLES);
        read = read.min(started.elapsed());
        size = fs::metadata(&path).unwrap().len();
        fs::remove_file(path).unwrap();
    }
    (write, read, size)
}

fn main() {
    let smoke = !std::env::args().any(|a| a == "--bench");
    let n = std::env::var("NORN_BENCH_CHUNKS")
        .ok()
        .and_then(|v| v.parse().ok());
    let n = n.unwrap_or(if smoke { 4 } else { 400 });
    let chunks: Vec<Chunk> = (0..n as u64).map(chunk).collect();
    let dir = std::env::temp_dir().join(format!("norn-bench-crypt-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let plain = StorageOptions::default();
    let mut sealed = plain;
    sealed.keyring = Some(Keyring::parse(KEY).unwrap().leak());
    let samples = (n * SAMPLES) as f64;
    println!(
        "| {n} chunks of {SAMPLES} f64 samples | write Msamples/s | read Msamples/s | bytes |"
    );
    println!("| --- | ---: | ---: | ---: |");
    for (name, opts) in [("plain", plain), ("encrypted", sealed)] {
        let (w, r, size) = run(&dir, &chunks, opts);
        let rate = |d: Duration| samples / d.as_secs_f64() / 1e6;
        println!("| {name} | {:.1} | {:.1} | {size} |", rate(w), rate(r));
    }
    fs::remove_dir_all(&dir).unwrap();
}
