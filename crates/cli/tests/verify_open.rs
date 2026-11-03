//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl verify` and `compact` on a stream whose data is all in its open
//! segment, or not in any segment yet: a short run's state. Verify used to
//! report `segments=0 chunks=0 samples=0` and exit 0 over half a million stored
//! samples.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod field;

use std::time::{Duration, Instant};

use field::{Field, now_nanos, sample};

/// Samples reach the open segment 50 ms after they arrive.
const QUICK_CHUNKS: &str = "[storage]\nmax_chunk_age_ms = 50\n";

/// Waits until `nornctl streams` shows nothing buffered for `plant`.
fn wait_until_stored(f: &Field) {
    let started = Instant::now();
    loop {
        let streams = f.ok(&["streams"]);
        if !streams.contains("buffered=") {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "never stored: {streams}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_reads_the_chunks_already_written_to_the_open_segment() {
    let f = Field::start(QUICK_CHUNKS).await;
    f.seed(&["t"]);
    let start = now_nanos();
    let list = (0..500)
        .map(|n| sample("t", start + n * 1_000, n))
        .collect();
    f.append(1, list).await;
    wait_until_stored(&f);
    let verified = f.ok(&["verify", "plant"]);
    assert!(
        verified.contains("segments=0 "),
        "nothing is sealed: {verified}"
    );
    assert!(verified.contains(" samples=500 "), "{verified}");
    assert!(!verified.contains(" chunks=0 "), "{verified}");
    assert!(verified.contains("damaged_chunks=0"), "{verified}");
    assert!(verified.contains("buffered=0"), "{verified}");
    let compacted = f.ok(&["compact", "plant"]);
    assert!(
        compacted.contains("nothing to compact: no sealed segment"),
        "{compacted}"
    );
    assert!(!compacted.contains("chunks=0->0"), "{compacted}");
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_verify_that_examined_nothing_while_samples_are_held_fails() {
    // The default chunk age is a minute: these stay in the write buffers.
    let f = Field::start("").await;
    f.seed(&["t"]);
    let start = now_nanos();
    let list = (0..10).map(|n| sample("t", start + n, n)).collect();
    f.append(1, list).await;
    let out = f.nornctl(&["verify", "plant"], Duration::from_secs(60));
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(out.status.code(), Some(1), "{stdout} {stderr}");
    assert!(stdout.contains(" chunks=0 "), "{stdout}");
    assert!(stdout.contains("buffered=10"), "{stdout}");
    assert!(stderr.contains("examined nothing"), "{stderr}");
    assert!(stderr.contains("max_chunk_age"), "{stderr}");
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verifying_an_empty_stream_passes_because_there_is_nothing_to_examine() {
    let f = Field::start("").await;
    f.seed(&["t"]);
    let verified = f.ok(&["verify", "plant"]);
    assert!(verified.contains(" chunks=0 samples=0 "), "{verified}");
    f.stop().await;
}
