//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The field run's replay: samples appended over a few seconds at wall-clock
//! times, then `nornctl replay create all --from F --to T --speed max` and
//! `nornctl subscribe replay ID --resume`, which must print every sample and
//! then ENDED, rather than one event and nothing more.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod field;

use std::time::Duration;

use field::{Field, now_nanos, sample};

const SIGNALS: [&str; 3] = ["a", "b", "c"];
const ROUNDS: i64 = 4;
const PER_ROUND: i64 = 400;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_max_speed_replay_of_a_recent_window_prints_every_sample_then_ended() {
    let f = Field::start("").await;
    f.seed(&SIGNALS);
    let first = now_nanos();
    let mut sequence = 0;
    for round in 0..ROUNDS {
        let base = now_nanos();
        let list = (0..PER_ROUND)
            .map(|n| {
                sequence += 1;
                let id = SIGNALS[n as usize % SIGNALS.len()];
                sample(id, base + n * 1_000, sequence)
            })
            .collect();
        f.append(round + 1, list).await;
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    let last = now_nanos();
    // Whole seconds, which nornctl parses exactly, bracketing every sample.
    let from = (first / 1_000_000_000).to_string();
    let to = (last / 1_000_000_000 + 2).to_string();
    let made = f.ok(&[
        "replay", "create", "all", "--from", &from, "--to", &to, "--speed", "max", "--id", "window",
    ]);
    assert!(made.contains("PAUSED"), "{made}");
    let out = f.nornctl(
        &["subscribe", "replay", "window", "--resume"],
        Duration::from_secs(60),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.first().is_some_and(|l| l.ends_with("RUNNING")),
        "{stdout}"
    );
    assert!(
        lines.last().is_some_and(|l| l.ends_with("ENDED")),
        "never ENDED: {} lines, the last {:?}; stderr: {stderr}",
        lines.len(),
        lines.last()
    );
    let samples = lines.iter().filter(|l| l.contains(" GOOD seq=")).count();
    assert_eq!(samples as i64, ROUNDS * PER_ROUND, "{stderr}");
    for (n, line) in lines.iter().enumerate() {
        assert!(line.starts_with(&format!("#{} ", n + 1)), "{line}");
    }
    f.stop().await;
}
