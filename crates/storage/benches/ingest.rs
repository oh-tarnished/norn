//! SPEC §15 model on disk: 16,000 signals at 1 Hz for one hour (57.6 million
//! samples) through the real writer, compaction and reader.
//!
//! `cargo bench -p norn-storage --bench ingest` prints a Markdown report.
//! Single-threaded on purpose: this is one encoder's floor, not the write
//! path's parallel rate. Sizes are file sizes, so they include every header,
//! CRC, directory and trailer. `cargo test --all-targets` runs a tiny smoke
//! scale; `NORN_BENCH_SIGNALS` and `NORN_BENCH_SECONDS` override the scale.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "../../codec/benches/shapes/gen.rs"]
#[allow(dead_code)]
mod gen_;
#[path = "ingest/series.rs"]
mod series;

use std::fs;
use std::path::Path;
use std::time::Instant;

use norn_core::{Config, StreamId, Timestamp};
use norn_storage::{Chunk, SegmentReader, StorageOptions, StreamWriter, compact, live_segments};
use series::{Population, SHAPES};

/// Samples a year holds at this workload (SPEC §15): 16,000 × 86,400 × 365.
const YEAR_SAMPLES: f64 = 504_576_000_000.0;
/// Bytes one buffered sample costs in memory (ts, seq, quality, widest value).
const SAMPLE_BYTES: u64 = 8 + 8 + 2 + 8;

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn dir_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum()
}

fn main() {
    let smoke = !std::env::args().any(|a| a == "--bench");
    let signals = env("NORN_BENCH_SIGNALS", if smoke { 20 } else { 16_000 });
    let seconds = env("NORN_BENCH_SECONDS", if smoke { 600 } else { 3_600 });
    let config = Config::default();
    let opts = StorageOptions::from_config(&config);
    // What the ingest budget can buffer per signal decides the chunk size.
    let per_signal = config.memory().ingest_bytes / signals as u64 / SAMPLE_BYTES;
    let window = usize::try_from(per_signal).unwrap().clamp(1, seconds);
    let total = (signals * seconds) as f64;

    let root = std::env::temp_dir().join(format!("norn-ingest-bench-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let stream = StreamId::new(1);
    let population = Population::new(signals);

    let mut w = StreamWriter::open(&root, stream, opts).unwrap();
    let (mut writing, mut syncing) = (0.0f64, 0.0f64);
    let mut raw_per_shape = [0u64; SHAPES.len()];
    for start in (0..seconds).step_by(window) {
        let end = (start + window).min(seconds);
        for signal in 0..signals {
            let samples: Vec<_> = (start..end).map(|t| population.sample(signal, t)).collect();
            let chunk = Chunk::from_samples(&samples).unwrap();
            let t = Instant::now();
            let done = w.append(&chunk).unwrap();
            writing += t.elapsed().as_secs_f64();
            raw_per_shape[signal % SHAPES.len()] += done.chunk.frame_len();
        }
        let t = Instant::now();
        w.sync().unwrap();
        syncing += t.elapsed().as_secs_f64();
    }
    let t = Instant::now();
    w.close().unwrap();
    syncing += t.elapsed().as_secs_f64();

    let inputs: Vec<_> = live_segments(&root)
        .unwrap()
        .into_iter()
        .map(|s| s.path)
        .collect();
    let ingest_bytes = dir_bytes(&root);
    let ingest_chunks = signals * seconds.div_ceil(window);

    let t = Instant::now();
    let done = compact(&root, &inputs, &opts).unwrap();
    let compacting = t.elapsed().as_secs_f64();
    let compact_bytes = dir_bytes(&root);

    let mut r = SegmentReader::open(&done.output.path, opts).unwrap();
    let opened = r.bytes_read();
    let mut compact_per_shape = [0u64; SHAPES.len()];
    for c in r.chunks() {
        compact_per_shape[usize::try_from(c.signal.get()).unwrap() % SHAPES.len()] += c.frame_len();
    }
    let t = Instant::now();
    let mut read = 0usize;
    for s in r.samples(None, Timestamp::MIN, Timestamp::MAX) {
        s.unwrap();
        read += 1;
    }
    let reading = t.elapsed().as_secs_f64();
    assert_eq!(read as f64, total, "every sample came back");

    let bps = |b: u64| b as f64 / total;
    let year = |b: u64| bps(b) * YEAR_SAMPLES / 1e12;
    println!(
        "{signals} signals, {seconds} s, {total} samples; ingest chunks of {window} samples\n"
    );
    println!(
        "| Layout | Segments | Chunks | Bytes on disk | Bytes/sample | Year at 16,000 × 1 Hz |"
    );
    println!("| --- | ---: | ---: | ---: | ---: | ---: |");
    println!(
        "| ingest, zstd 3 | {} | {ingest_chunks} | {ingest_bytes} | {:.4} | {:.2} TB |",
        inputs.len(),
        bps(ingest_bytes),
        year(ingest_bytes)
    );
    println!(
        "| compacted, smallest of zstd 3-6 | 1 | {} | {compact_bytes} | {:.4} | {:.2} TB |\n",
        done.output.chunks,
        bps(compact_bytes),
        year(compact_bytes)
    );
    println!("| Step | Seconds | Samples/s |");
    println!("| --- | ---: | ---: |");
    for (step, s) in [
        ("encode and append", writing),
        ("fsync (per window) and seal", syncing),
        ("write total", writing + syncing),
        ("compaction", compacting),
        ("streaming read, all samples", reading),
    ] {
        println!("| {step} | {s:.2} | {:.0} |", total / s);
    }
    println!("\nOpening the compacted segment read {opened} bytes.\n");
    println!("| Shape | Ingest bytes/sample | Compacted bytes/sample |");
    println!("| --- | ---: | ---: |");
    let n = total / SHAPES.len() as f64;
    for (i, shape) in SHAPES.iter().enumerate() {
        println!(
            "| {shape} | {:.4} | {:.4} |",
            raw_per_shape[i] as f64 / n,
            compact_per_shape[i] as f64 / n
        );
    }
    let block = vec![0xa5u8; 1 << 20];
    let t = Instant::now();
    let crcs: u32 = (0..256).fold(0, |c, _| norn_storage::crc32c_extend(c, &block));
    let rate = 256.0 / t.elapsed().as_secs_f64();
    println!("\nCRC-32C: {rate:.0} MiB/s (check {crcs:08x})");
    let _ = fs::remove_dir_all(&root);
}
