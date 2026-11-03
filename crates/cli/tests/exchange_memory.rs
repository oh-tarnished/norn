//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Export and import hold memory flat as the data grows (SPEC §15, §44, §45).
//!
//! Unix only: it samples resident memory with `ps -o rss=`, which Windows lacks
//! (docs/guides/operations.md, "Platforms").
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::process::{Command, Stdio};
use std::time::Duration;

use norn_adapter::{ImportOptions, import, time};
use norn_adapter_testkit::{MemorySource, catalog, signal_name};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, Sample, Value, ValueType};
use norn_protocol::telemetry::AcknowledgementLevel;
use support::Running;

fn batch(from: i64, n: i64) -> Vec<Sample> {
    (from..from + n)
        .map(|i| Sample {
            signal: signal_name(ValueType::Double),
            observe_time: Some(time::from_nanos(i * 1_000_000)),
            value: Some(Value {
                kind: Some(Kind::DoubleValue(i as f64)),
            }),
            quality: Some(Quality {
                severity: 1,
                substatus: 0,
            }),
            sequence: 0,
        })
        .collect()
}

async fn grow(daemon: &Running, from: i64, n: i64) {
    let options = ImportOptions::default().ack(AcknowledgementLevel::Buffered);
    let channel = daemon.channel().await;
    let mut memory = MemorySource {
        catalog: catalog(),
        samples: Some(batch(from, n)),
    };
    import(channel, &mut memory, &options).await.unwrap();
}

/// Runs `nornctl` and returns its peak resident size in KiB, sampled from `ps`.
fn peak_rss(daemon: &Running, words: &[&str]) -> u64 {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args([
            "--endpoint",
            &daemon.endpoint.to_string(),
            "--timeout",
            "60",
        ])
        .args(words)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id().to_string();
    let mut peak = 0;
    while child.try_wait().unwrap().is_none() {
        let ps = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&ps.stdout);
        peak = peak.max(text.trim().parse().unwrap_or(0));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(child.wait().unwrap().success(), "{words:?}");
    peak
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_times_the_samples_does_not_grow_a_clients_memory_with_it() {
    const SMALL: i64 = 100_000;
    let daemon = Running::start().await;
    grow(&daemon, 0, SMALL).await;
    let mut small = Vec::new();
    for format in ["csv", "parquet", "mcap"] {
        let path = daemon.dir.0.join(format!("small.{format}"));
        let path = path.to_str().unwrap();
        small.push(peak_rss(
            &daemon,
            &["export", "all", "--format", format, "--output", path],
        ));
    }
    grow(&daemon, SMALL, SMALL * 3).await;
    for (format, small) in ["csv", "parquet", "mcap"].iter().zip(small) {
        let path = daemon.dir.0.join(format!("large.{format}"));
        let path = path.to_str().unwrap();
        let large = peak_rss(
            &daemon,
            &["export", "all", "--format", format, "--output", path],
        );
        assert!(small > 0, "ps saw nothing, so this test proved nothing");
        // Parquet's row group fills past the small run, so allow for one group.
        assert!(
            large < small * 2 + 64 * 1024,
            "{format}: {small} KiB, then {large} KiB"
        );
        let size = std::fs::metadata(path).unwrap().len();
        assert!(size > 0);
        // Reading it back into the same daemon sends nothing, but reads everything.
        let imported = peak_rss(&daemon, &["import", path, "--ack", "buffered"]);
        assert!(
            imported < 128 * 1024,
            "{format}: import took {imported} KiB"
        );
    }
    daemon.stop().await;
}
