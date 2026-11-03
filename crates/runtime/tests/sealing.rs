//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Sealing by age, on a fake clock: a quiet stream's open segment is sealed once
//! it has been open `max_segment_open_time`, and a clean close seals it too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common_ingest;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common_ingest::{Dir, Rig, rig};
use norn_core::{Config, MaintenanceConfig, StorageConfig, Timestamp};
use norn_runtime::ingest::AckLevel;
use norn_runtime::maintenance::Maintenance;
use norn_runtime::{Budget, Clock};
use tokio_util::sync::CancellationToken;

/// Ten minutes open, an hour of chunk age: the test decides when each happens.
const OPEN: Duration = Duration::from_secs(600);

fn config() -> Config {
    let mut storage = StorageConfig::default();
    storage.max_chunk_age = Duration::from_secs(3600);
    storage.max_segment_open_time = OPEN;
    Config::builder().storage(storage).build().unwrap()
}

/// (sealed segments, chunks in open segments) as the index has them.
fn shape(r: &Rig) -> (usize, usize) {
    let snapshot = r.ingest.index().unwrap().snapshot();
    (
        snapshot.segments(r.stream).len(),
        snapshot.tail_chunks(r.stream),
    )
}

/// Polls `shape` for up to five seconds of real time.
async fn reaches(r: &Rig, want: (usize, usize)) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if shape(r) == want {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// Stores 100 samples in a new, open segment, at about 3,601 s on the clock.
async fn stored_open(r: &Rig) {
    let mut admitted = r.batch(0..100, AckLevel::Persisted);
    admitted.completion.take().unwrap().await.unwrap().unwrap();
    // Past the chunk age: the commit thread hands the samples to the store thread.
    r.clock.advance(Duration::from_secs(3_601));
    assert!(reaches(r, (0, 1)).await, "never stored: {:?}", shape(r));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_segment_open_past_the_limit_is_asked_to_seal() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(), None);
    assert_eq!(r.ingest.seal_aged(r.clock.now()), 0, "nothing is open");
    stored_open(&r).await;
    let young = r.clock.now().as_nanos() + 599 * 1_000_000_000;
    assert_eq!(r.ingest.seal_aged(Timestamp::from_nanos(young)), 0);
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(shape(&r), (0, 1), "a younger segment stays open");
    let old = r.clock.now().as_nanos() + 600 * 1_000_000_000;
    assert_eq!(r.ingest.seal_aged(Timestamp::from_nanos(old)), 1);
    assert!(reaches(&r, (1, 0)).await, "not sealed: {:?}", shape(&r));
    let snapshot = r.ingest.index().unwrap().snapshot();
    assert_eq!(snapshot.segments(r.stream).first().unwrap().samples, 100);
    assert_eq!(
        r.ingest.stats(r.stream).samples,
        100,
        "nothing lost or doubled"
    );
    r.ingest.close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn maintenance_seals_a_quiet_stream_on_the_injected_clock() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(), None);
    stored_open(&r).await;
    let clock: Arc<dyn Clock> = Arc::new(r.clock.clone());
    let memory = Budget::new("chunk_cache_bytes", 64 << 20, Arc::clone(&clock));
    let mut schedule = MaintenanceConfig::default();
    schedule.background = false;
    let worker = Arc::new(Maintenance::new(
        Arc::clone(&r.ingest),
        Arc::clone(&r.registry),
        memory,
        clock,
        schedule,
    ));
    let token = CancellationToken::new();
    let task = tokio::spawn(Arc::clone(&worker).run(token.clone()));
    // Nine minutes, a minute at a time: the worker looks every 150 s and finds
    // the segment younger than ten minutes each time.
    for _ in 0..9 {
        r.clock.advance(Duration::from_secs(60));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(shape(&r), (0, 1), "sealed early");
    let mut sealed = false;
    for _ in 0..10 {
        r.clock.advance(Duration::from_secs(60));
        if reaches_soon(&r).await {
            sealed = true;
            break;
        }
    }
    assert!(sealed, "maintenance never sealed it: {:?}", shape(&r));
    token.cancel();
    task.await.unwrap();
    r.ingest.close();
}

/// Whether the segment seals within the store thread's one-second tick.
async fn reaches_soon(r: &Rig) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(1_500) {
        if shape(r) == (1, 0) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_close_seals_the_open_segment() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(), None);
    stored_open(&r).await;
    r.ingest.close();
    assert_eq!(shape(&r), (1, 0), "closing must seal what is open");
}
