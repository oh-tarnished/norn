//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl status`'s published limits, `inspect`, `stats` and `benchmark`:
//! the real binary against a real daemon.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod field;

use std::time::{Duration, Instant};

use field::{Field, now_nanos, sample};

/// Samples reach the open segment 50 ms after they arrive.
const QUICK_CHUNKS: &str = "[storage]\nmax_chunk_age_ms = 50\n";
const MINUTE: Duration = Duration::from_secs(60);

/// `field=` from `line`, as text.
fn field_of<'a>(line: &'a str, name: &str) -> &'a str {
    line.split(' ')
        .find_map(|w| w.strip_prefix(name)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {name} in {line}"))
}

fn wait_until_stored(f: &Field) {
    let started = Instant::now();
    while f.ok(&["streams"]).contains("buffered=") {
        assert!(started.elapsed() < Duration::from_secs(20), "never stored");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn value<'a>(out: &'a str, key: &str) -> &'a str {
    out.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
        .unwrap_or_else(|| panic!("no {key} in {out}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_prints_the_published_size_limits() {
    let f = Field::start("").await;
    let out = f.ok(&["status"]);
    assert_eq!(value(&out, "limits.max_message_bytes"), "262144");
    assert_eq!(value(&out, "limits.max_batch_create_requests"), "1000");
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_shows_the_layout_of_the_open_segment() {
    let f = Field::start(QUICK_CHUNKS).await;
    f.seed(&["t"]);
    let start = now_nanos();
    let list = (0..500)
        .map(|n| sample("t", start + n * 1_000, n + 1))
        .collect();
    f.append(1, list).await;
    wait_until_stored(&f);

    let full = f.ok(&["inspect", "plant"]);
    let total = full.lines().last().unwrap();
    assert!(total.starts_with("total segments=1 "), "{full}");
    assert_eq!(field_of(total, "samples"), "500", "{full}");
    let ratio: f64 = field_of(total, "ratio").parse().unwrap();
    assert!(ratio > 1.0, "500 regular doubles must compress: {full}");
    for column in ["TIMESTAMPS", "VALUES", "QUALITIES", "SEQUENCES"] {
        assert!(full.contains(&format!("  {column} codec=")), "{full}");
    }
    let segment_line = full.lines().nth(1).unwrap();
    assert_eq!(field_of(segment_line, "signals"), "1", "{full}");
    assert_eq!(field_of(segment_line, "damaged_chunks"), "0", "{full}");
    assert_ne!(field_of(segment_line, "from"), "-", "{full}");

    let name = segment_line.split(' ').next().unwrap();
    let id = name.rsplit('/').next().unwrap();
    let one = f.ok(&["inspect", "plant", id]);
    assert!(one.contains("  VALUES codec="), "{one}");
    let basic = f.ok(&["inspect", "plant", "--basic"]);
    assert!(!basic.contains("codec="), "{basic}");
    assert!(!basic.contains("ratio="), "{basic}");
    assert!(basic.contains(" samples=500 "), "{basic}");

    let missing = f.nornctl(&["inspect", "plant", "ffff-g0000"], MINUTE);
    assert!(!missing.status.success());
    let absent = f.nornctl(&["inspect", "nothing"], MINUTE);
    assert!(!absent.status.success());

    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_reads_sealed_segments_as_well_as_the_open_one() {
    let f = Field::start("[storage]\nmax_chunk_age_ms = 50\nmax_segment_bytes = 256\n").await;
    f.seed(&["t"]);
    let start = now_nanos();
    for batch in 0..30 {
        let list = (0..500)
            .map(|n| {
                let sequence = batch * 500 + n + 1;
                sample("t", start + sequence * 1_000, sequence)
            })
            .collect();
        f.append(batch + 1, list).await;
        wait_until_stored(&f);
    }
    let full = f.ok(&["inspect", "plant"]);
    assert!(full.contains(" sealed=true "), "{full}");
    assert!(full.contains(" sealed=false "), "{full}");
    let total = full.lines().last().unwrap();
    assert_eq!(field_of(total, "samples"), "15000", "{full}");
    // More sealed segments than one FULL page: the pages are all followed.
    let segments: u64 = field_of(total, "segments").parse().unwrap();
    assert!(segments > 10, "a FULL page holds 10: {full}");
    assert_eq!(
        full.matches("damaged_chunks=0").count() as u64 + 2,
        full.lines().filter(|l| !l.starts_with("  ")).count() as u64,
        "{full}"
    );
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stats_counts_per_signal_and_per_stream_only_inside_the_window() {
    let f = Field::start(QUICK_CHUNKS).await;
    f.seed(&["a", "b"]);
    let start = now_nanos();
    let mut list: Vec<_> = (0..30).map(|n| sample("a", start + n, n + 1)).collect();
    list.extend((0..10).map(|n| sample("b", start + n, n + 1)));
    f.append(1, list).await;
    wait_until_stored(&f);

    let from = (start / 1_000_000_000 - 9).to_string();
    let to = (start / 1_000_000_000 + 11).to_string();
    let out = f.ok(&["stats", "--from", &from, "--to", &to]);
    let line = |prefix: &str| {
        out.lines()
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("no {prefix} in {out}"))
    };
    let a = line("signal entities/dev/signals/a ");
    assert_eq!(field_of(a, "stream"), "streams/plant");
    assert_eq!(field_of(a, "samples"), "30");
    assert_eq!(field_of(a, "rate"), "1.500/s");
    assert_eq!(
        field_of(line("signal entities/dev/signals/b "), "samples"),
        "10"
    );
    let stream = line("stream streams/plant ");
    assert_eq!(field_of(stream, "signals"), "2");
    assert_eq!(field_of(stream, "samples"), "40");
    assert_eq!(field_of(stream, "rate"), "2.000/s");
    let stored: i64 = field_of(stream, "stored_bytes").parse().unwrap();
    assert!(stored > 0, "{out}");

    // A window before the samples counts none of them, and one signal alone
    // is selected by name.
    let early = f.ok(&["stats", "--from", "1000", "--to", "2000"]);
    assert!(early.contains(" samples=0 "), "{early}");
    let one = f.ok(&[
        "stats",
        "entities/dev/signals/b",
        "--from",
        &from,
        "--to",
        &to,
    ]);
    assert!(!one.contains("signals/a"), "{one}");
    let backwards = f.nornctl(&["stats", "--from", "2000", "--to", "1000"], MINUTE);
    assert!(!backwards.status.success());
    f.stop().await;
}
