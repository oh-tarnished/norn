//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! StateAt through real gRPC over a Unix socket: docs/performance.md's numbers.
//!
//! `cargo bench -p norn-server --bench state`. Starts the real `norn`, loads a small
//! data set, then measures closed-loop on one client: random instants, scrubbing,
//! a stand-in for each checkpoint interval (SPEC §24), and a replay at maximum
//! speed, all of it, to one subscriber. `NORN_BENCH_SIGNALS` and `NORN_BENCH_SECONDS` change the scale. `cargo test
//! --all-targets` runs a tiny smoke scale. Data lives under a temp directory removed
//! at the end: about 2.2 bytes a sample.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "append/driver.rs"]
#[allow(dead_code)]
mod driver;
#[path = "query/load.rs"]
mod load;
#[path = "query/measure.rs"]
#[allow(dead_code)]
mod measure;
#[path = "state/replaying.rs"]
mod replaying;
#[path = "append/setup.rs"]
#[allow(dead_code)]
mod setup;
#[path = "state/timing.rs"]
mod timing;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use driver::connect;
use measure::{disk, rss_now};
use norn_protocol::state::ComputeStateRequest;
use setup::seed;
use timing::{at, changes_since, row, state, timed};

const START: i64 = 1_700_000_000;

fn scale(name: &str, fallback: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// A spread of instants that is the same every run.
fn instant(round: usize, seconds: i64) -> i64 {
    START + 1 + (round as i64 * 2_654_435_761).rem_euclid(seconds - 1)
}

fn all(t: i64) -> ComputeStateRequest {
    ComputeStateRequest {
        parent: "entities/-".into(),
        state_time: at(t),
        ..ComputeStateRequest::default()
    }
}

#[tokio::main]
async fn main() {
    let smoke = !std::env::args().any(|a| a == "--bench");
    let signals = if smoke {
        5
    } else {
        scale("NORN_BENCH_SIGNALS", 2_000)
    };
    let seconds = if smoke {
        600
    } else {
        scale("NORN_BENCH_SECONDS", 3_600)
    };
    let root = std::env::temp_dir().join(format!("norn-sbench-{}", std::process::id()));
    let socket = root.join("ipc/n.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .arg("--listen")
        .arg(format!("unix:{}", socket.display()))
        .args(["--log-level", "warn"])
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let channel = connect(&socket).await;
    let names = seed(&channel, signals as usize).await;
    let began = Instant::now();
    load::load(&channel, &names, START, seconds).await;
    println!(
        "loaded {} samples in {:.1?}",
        signals * seconds,
        began.elapsed()
    );
    if !smoke {
        // Past `max_chunk_age`, so these read chunks and not the buffers.
        tokio::time::sleep(Duration::from_secs(62)).await;
    }
    let pid = daemon.id();
    let rounds = |n: usize| if smoke { 3 } else { n };
    println!("| Measurement | Rounds | p50 | p99 |");
    println!("| --- | ---: | ---: | ---: |");
    let c = &channel;
    let mut took = timed(rounds(500), |r| async move {
        let request = ComputeStateRequest {
            filter: format!("id = \"s{}\"", r as i64 % signals),
            ..all(instant(r, seconds))
        };
        assert_eq!(state(c, request).await, 1);
    })
    .await;
    row("StateAt, one signal, random T", &mut took);
    let mut took = timed(rounds(100), |r| async move {
        assert_eq!(state(c, all(instant(r, seconds))).await, signals as usize);
    })
    .await;
    row("StateAt, all signals, random T", &mut took);
    let mut took = timed(rounds(100), |_| async move {
        state(c, all(START + seconds + 10)).await;
    })
    .await;
    row("StateAt, all signals, after the last sample", &mut took);
    let steps = rounds(200);
    let mut took = timed(steps, |r| async move {
        state(c, all(START + 60 + r as i64 * 5)).await;
    })
    .await;
    row("Scrubbing, all signals, 5 s steps", &mut took);
    let mut took = timed(steps, |r| async move {
        let request = ComputeStateRequest {
            filter: "id = \"s0\"".into(),
            ..all(START + 60 + r as i64 * 5)
        };
        state(c, request).await;
    })
    .await;
    row("Scrubbing, one signal, 5 s steps", &mut took);
    for interval in [60, 300, 900, 3_600] {
        let shots = rounds(if interval > 300 { 10 } else { 50 });
        let mut took = timed(shots, |r| async move {
            let t = instant(r, seconds);
            let since = START + (t - START) / interval * interval + 1;
            changes_since(c, since.min(t), t).await;
        })
        .await;
        row(
            &format!("Checkpoint stand-in, every {interval} s"),
            &mut took,
        );
    }
    // The whole set, or its first NORN_BENCH_REPLAY_SECONDS when that is shorter.
    let window = scale("NORN_BENCH_REPLAY_SECONDS", seconds).min(seconds);
    for round in 0..rounds(3) {
        let (samples, seconds_taken) = replaying::replay(c, START, START + window).await;
        assert_eq!(samples as i64, signals * window, "replay lost samples");
        let rate = samples as f64 / seconds_taken;
        println!(
            "replay at maximum speed, round {round}: {samples} samples in {seconds_taken:.2} s, {rate:.0} samples/s"
        );
    }
    println!("daemon RSS {} MiB", rss_now(pid) / 1024);
    println!("data on disk {} MiB", disk(&root.join("data")) >> 20);
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
