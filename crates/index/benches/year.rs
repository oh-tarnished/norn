//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A year of segment directories, indexed: SPEC §15's 16,000 signals at the
//! chunk density docs/reference/storage.md measured.
//!
//! `cargo bench -p norn-index --bench year` prints a Markdown report. Segment
//! files are real and pass the real reader, but hold a header, a directory and a
//! trailer with no chunk bodies: the index never reads a body, and 0.61 TB of
//! them would measure nothing else. Only the two chunk densities differ:
//!
//! - **compacted**: 1 chunk per signal per hour, 1 segment an hour, a full year.
//! - **ingest**: 23 chunks per signal per hour, 4 segments an hour. A year is 141
//!   GB of directories, so by default this measures one week and the report
//!   says so; `NORN_BENCH_INGEST_HOURS=8760` runs it whole, disk permitting.
//!
//! `cargo test --all-targets` runs a tiny smoke scale. `NORN_BENCH_SIGNALS` and
//! `NORN_BENCH_HOURS` override it.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "year/gen.rs"]
mod gen_;
#[path = "year/lookups.rs"]
mod lookups;

use std::fs;
use std::path::Path;
use std::time::Instant;

use gen_::Layout;
use norn_index::{IndexOptions, TemporalIndex};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn dir_bytes(dir: &Path) -> (u64, u64) {
    let (mut files, mut bytes) = (0, 0);
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        files += 1;
        bytes += e.metadata().unwrap().len();
    }
    (files, bytes)
}

/// Resident memory of this process in bytes, from `ps`; 0 where it has none.
fn rss() -> u64 {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    text.ok().and_then(|t| t.parse::<u64>().ok()).unwrap_or(0) * 1024
}

fn run(layout: &Layout, samples: usize, opts: IndexOptions) {
    let root = std::env::temp_dir().join(format!(
        "norn-index-bench-{}-{}",
        std::process::id(),
        layout.name
    ));
    let _ = fs::remove_dir_all(&root);
    let t = Instant::now();
    let segment_bytes = gen_::write(&root, layout);
    println!(
        "\n### {}: {} hours, {} segments, {} chunks\n",
        layout.name,
        layout.hours,
        layout.segments(),
        layout.chunks()
    );
    println!(
        "Generated {:.1} GB of directories in {:.1} s.\n",
        segment_bytes as f64 / 1e9,
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let (index, report) = TemporalIndex::open(&root, opts).unwrap();
    let rebuild = t.elapsed().as_secs_f64();
    assert!(report.streams[0].rebuilt());
    assert_eq!(report.streams[0].read as u64, layout.segments());
    let stats = index.stats();
    assert_eq!(stats.segment_opens, layout.segments());
    drop(index);

    let index_dir = root
        .join("index")
        .join(format!("{:016x}", gen_::STREAM.get()));
    let (files, disk) = dir_bytes(&index_dir);
    let summary = fs::metadata(index_dir.join("summary.nidx")).unwrap().len();

    let t = Instant::now();
    let (index, report) = TemporalIndex::open(&root, opts).unwrap();
    let load = t.elapsed().as_secs_f64();
    assert!(report.streams[0].clean(), "{report:?}");
    let list_memory = index.memory_bytes();

    println!("| Measure | Value |\n| --- | ---: |");
    println!(
        "| full rebuild | {rebuild:.2} s, {} threads |",
        opts.rebuild_threads
    );
    println!("| startup load | {:.1} ms |", load * 1e3);
    println!(
        "| in memory after load | {:.2} MiB |",
        list_memory as f64 / 1048576.0
    );
    println!(
        "| summary on disk | {:.2} MiB |",
        summary as f64 / 1048576.0
    );
    println!(
        "| sidecars on disk | {:.2} GiB in {} files |",
        (disk - summary) as f64 / 1073741824.0,
        files - 1
    );
    println!(
        "| bytes per chunk on disk | {:.1} |",
        disk as f64 / layout.chunks() as f64
    );

    println!(
        "\n| Lookup | Runs | Chunks each | p50 | p99 | Max | Cache hits |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |"
    );
    // The rebuild left sidecars unsynced by design. Flush them first, so the
    // lookups measure the index and not the OS's writeback of that rebuild.
    let flushed = std::process::Command::new("sync").status();
    assert!(flushed.is_ok_and(|s| s.success()) || cfg!(windows));
    let segment_opens = index.stats().segment_opens;
    lookups::all(&index, layout, samples);
    assert_eq!(
        index.stats().segment_opens,
        segment_opens,
        "a lookup opened a segment"
    );
    println!(
        "\nIn memory after lookups: {:.2} MiB accounted (budget {:.0} MiB), process RSS {:.0} MiB.",
        index.memory_bytes() as f64 / 1048576.0,
        opts.memory_budget_bytes as f64 / 1048576.0,
        rss() as f64 / 1048576.0
    );
    assert!(index.memory_bytes() <= opts.memory_budget_bytes);
    drop(index);
    fs::remove_dir_all(&root).unwrap();
}

fn main() {
    // `cargo bench` passes `--bench`. `cargo test --all-targets` passes neither
    // that nor `--test` to a harness-less target, so anything else is a smoke run.
    let smoke = !std::env::args().any(|a| a == "--bench");
    let signals = env("NORN_BENCH_SIGNALS", if smoke { 200 } else { 16_000 });
    let hours = env("NORN_BENCH_HOURS", if smoke { 48 } else { 8_760 });
    let ingest_hours = env("NORN_BENCH_INGEST_HOURS", if smoke { 4 } else { 168 });
    let samples = if smoke { 20 } else { 1_000 };
    let opts = IndexOptions::default();
    run(&Layout::compacted(signals, hours), samples, opts);
    run(&Layout::ingest(signals, ingest_hours), samples, opts);
}
