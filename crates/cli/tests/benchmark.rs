//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl benchmark`: the real binary against a real daemon.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod field;

use std::time::Duration;

use field::Field;

const MINUTE: Duration = Duration::from_secs(60);

fn value<'a>(out: &'a str, key: &str) -> &'a str {
    out.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
        .unwrap_or_else(|| panic!("no {key} in {out}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn benchmark_runs_on_its_own_stream_and_refuses_one_holding_samples() {
    let f = Field::start("").await;
    let words = [
        "benchmark",
        "--signals",
        "3",
        "--rate",
        "2000",
        "--duration",
        "1s",
        "--queries",
        "2",
    ];
    let out = f.ok(&words);
    assert_eq!(value(&out, "stream"), "streams/nornctl-benchmark");
    let appended: u64 = value(&out, "append.samples").parse().unwrap();
    assert!(appended > 0, "{out}");
    assert_eq!(value(&out, "append.rejected_batches"), "0", "{out}");
    let rate: f64 = value(&out, "append.samples_per_second").parse().unwrap();
    assert!(rate > 0.0 && rate < 4000.0, "paced to about 2000: {out}");
    let (p50, p99): (f64, f64) = (
        value(&out, "append.ack_p50_ms").parse().unwrap(),
        value(&out, "append.ack_p99_ms").parse().unwrap(),
    );
    assert!(0.0 < p50 && p50 <= p99, "{out}");
    let read: u64 = value(&out, "query.samples").parse().unwrap();
    assert_eq!(read, appended * 2, "two reads of everything: {out}");

    let again = f.nornctl(&words, MINUTE);
    assert_eq!(again.status.code(), Some(1), "{again:?}");
    let told = String::from_utf8_lossy(&again.stderr);
    assert!(told.contains("--allow-non-empty"), "{told}");
    assert!(again.stdout.is_empty(), "nothing ran: {again:?}");
    let held = f.ok(&["streams"]);
    assert!(held.contains(&format!("samples={appended}")), "{held}");

    let mut allowed = words.to_vec();
    allowed.extend(["--allow-non-empty"]);
    let more = f.ok(&allowed);
    assert_ne!(value(&more, "append.samples"), "0", "{more}");

    // A run longer than --timeout is not cut off by it: the bound is per call.
    let long = f.ok(&[
        "--timeout",
        "1",
        "benchmark",
        "--stream",
        "long",
        "--signals",
        "1",
        "--rate",
        "500",
        "--duration",
        "2s",
        "--queries",
        "1",
    ]);
    assert_eq!(value(&long, "append.rejected_batches"), "0", "{long}");

    // Another stream, as fast as acknowledged, and a batch past the limits.
    let other = f.ok(&[
        "benchmark",
        "--stream",
        "elsewhere",
        "--signals",
        "1",
        "--rate",
        "max",
        "--duration",
        "200ms",
        "--queries",
        "0",
    ]);
    assert_eq!(value(&other, "stream"), "streams/elsewhere");
    assert!(
        f.nornctl(&["benchmark", "--batch", "100000000"], MINUTE)
            .status
            .code()
            == Some(1)
    );
    f.stop().await;
}
