//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The write path through real gRPC over a Unix socket: docs/performance.md's numbers.
//!
//! `cargo bench -p norn-server --bench append`. Starts the real `norn`, creates
//! 16,000 signals, and drives it open-loop, honouring `THROTTLE`. `cargo test
//! --all-targets` runs a tiny smoke scale (cargo passes `--bench` only to a real
//! bench run). Its data lives under a temp directory removed at the end: about
//! 1 GiB at full scale.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "append/driver.rs"]
mod driver;
#[path = "append/setup.rs"]
mod setup;

use std::process::{Command, Stdio};
use std::time::Duration;

use driver::{Load, connect, drive};
use norn_protocol::telemetry::AcknowledgementLevel::{Buffered, Durable};
use setup::{seed, watch_rss};

const LOADS: [Load; 4] = [
    Load {
        name: "sustained",
        rate: 16_000,
        batch: 1_600,
        secs: 30,
        level: Buffered,
    },
    Load {
        name: "sustained",
        rate: 16_000,
        batch: 1_600,
        secs: 30,
        level: Durable,
    },
    Load {
        name: "burst",
        rate: 120_000,
        batch: 2_400,
        secs: 15,
        level: Buffered,
    },
    Load {
        name: "burst",
        rate: 120_000,
        batch: 2_400,
        secs: 15,
        level: Durable,
    },
];

#[tokio::main]
async fn main() {
    let root = std::env::temp_dir().join(format!("norn-bench-{}", std::process::id()));
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
    let smoke = !std::env::args().any(|a| a == "--bench");
    let names = seed(&channel, if smoke { 20 } else { 16_000 }).await;
    println!(
        "| Load | Level | Offered/s | Achieved/s | Ack p50 | Ack p99 | Throttled | Rejected | Peak RSS |"
    );
    println!("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    let mut base = 0;
    for mut load in LOADS {
        load.secs = if smoke { 1 } else { load.secs };
        let (rss, stop) = watch_rss(daemon.id());
        let report = drive(&channel, &names, load, base).await;
        stop.cancel();
        base += i64::try_from(load.rate * load.secs).unwrap() * 2;
        let peak = rss.await.unwrap();
        println!(
            "| {} | {:?} | {} | {:.0} | {:.2?} | {:.2?} | {} | {} | {} MiB |",
            load.name,
            load.level,
            load.rate,
            report.rate,
            report.p50,
            report.p99,
            report.throttled,
            report.rejected,
            peak / 1024,
        );
        tokio::time::sleep(Duration::from_millis(if smoke { 10 } else { 2_000 })).await;
    }
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
