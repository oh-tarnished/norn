//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The history scenario, once per repeat on a fresh daemon:
//!
//! 1. load `signals × seconds` at 1 Hz through `AppendSamples` at `PERSISTED`,
//!    one batch in flight, and measure bytes per sample in the ingest layout;
//! 2. stop cleanly and time the restart;
//! 3. compact the stream, timed, and measure bytes per sample again;
//! 4. read it back: queries, StateAt, replay (`reads`), on compacted data;
//! 5. load `crash_seconds` more, `kill -9`, and time the recovery.
//!
//! Bytes are file sizes under the stream directory: every header, checksum,
//! directory, index sidecar and trailer counts; the metadata log does not.

use std::path::Path;
use std::time::Instant;

use crate::compact::compact;
use crate::daemon::Daemon;
use crate::ingest::EPOCH;
use crate::measure::disk;
use crate::profile::History;
use crate::reads::{queries, replays, states};
use crate::seed::seed;
use crate::stats::{Better, Metrics};
use crate::{load, recovery};

pub async fn run_history(base: &Path, h: &History, metrics: &mut Metrics) {
    let (mut daemon, _) = Daemon::start(base, "history").await;
    let names = seed(&daemon.channel, h.signals).await;
    let samples = (h.signals as i64 * h.seconds) as f64;
    let began = Instant::now();
    load::load(&daemon.channel, &names, EPOCH, h.seconds).await;
    let loading = began.elapsed().as_secs_f64();
    let mut put = |key: &str, unit: &str, better: Better, gated: bool, value: f64| {
        metrics.record(&format!("history.{key}"), unit, better, gated, value);
    };
    put(
        "load.samples_per_s",
        "samples/s",
        Better::Higher,
        true,
        samples / loading,
    );
    daemon.terminate();
    let ingested = disk(&daemon.streams()) as f64;
    put(
        "load.disk_bytes_per_sample",
        "bytes",
        Better::Lower,
        true,
        ingested / samples,
    );
    let clean = daemon.restart().await.as_secs_f64();
    put("restart.clean_s", "s", Better::Lower, true, clean);
    let (result, took) = compact(&daemon.channel).await;
    assert!(
        result.original_chunk_count > 0,
        "nothing was sealed to compact"
    );
    put("compaction.seconds", "s", Better::Lower, true, took);
    put(
        "compaction.samples_per_s",
        "samples/s",
        Better::Higher,
        false,
        samples / took,
    );
    let chunks = result.compacted_chunk_count as f64;
    put(
        "compaction.chunks_after",
        "chunks",
        Better::Lower,
        false,
        chunks,
    );
    let compacted = disk(&daemon.streams()) as f64;
    put(
        "compaction.disk_bytes_per_sample",
        "bytes",
        Better::Lower,
        true,
        compacted / samples,
    );
    println!(
        "history: {samples} samples loaded in {loading:.1} s, {:.3} -> {:.3} bytes/sample, \
         compaction {took:.2} s",
        ingested / samples,
        compacted / samples
    );
    queries(&daemon.channel, h, metrics).await;
    states(&daemon.channel, h, metrics).await;
    replays(&daemon.channel, h, metrics).await;
    recovery::crash(&mut daemon, &names, h, metrics).await;
}
