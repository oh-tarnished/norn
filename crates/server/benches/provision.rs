//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Provisioning a large deployment through real gRPC over a Unix socket:
//! docs/performance.md, "Provisioning".
//!
//! `cargo bench -p norn-server --bench provision`. For each scenario, starts a
//! fresh `norn`, then creates 1,000 entities and 63,000 signals (a deployment the size
//! of a 15,804-point device network at four signals a point) over `connections`
//! connections, timing entities and signals separately. `cargo test
//! --all-targets` runs a tiny smoke scale.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "provision/site.rs"]
mod site;
#[path = "provision/write.rs"]
mod write;
#[path = "provision/batch.rs"]
mod write_batch;

use std::time::Duration;

use site::{Daemon, Shape, connect, create_stream};
use write::{Mode, provision};

const SCENARIOS: [(Mode, usize); 7] = [
    (Mode::OnePerCall, 8),
    (Mode::OnePerCall, 16),
    (Mode::OnePerCall, 32),
    (Mode::OnePerCall, 64),
    (Mode::Batched(1_000), 1),
    (Mode::Batched(1_000), 4),
    (Mode::Batched(500), 8),
];

#[tokio::main]
async fn main() {
    let smoke = !std::env::args().any(|a| a == "--bench");
    let shape = if smoke {
        Shape {
            devices: 4,
            points: 25,
        }
    } else {
        Shape {
            devices: 1_000,
            points: 63,
        }
    };
    println!("{} entities, {} signals", shape.devices, shape.signals());
    println!("| Method | Connections | Entities | Signals | Signals/s | Total |");
    println!("| --- | ---: | ---: | ---: | ---: | ---: |");
    for (n, (mode, connections)) in SCENARIOS.into_iter().enumerate() {
        let daemon = Daemon::start(&n.to_string());
        let mut channels = Vec::with_capacity(connections);
        for _ in 0..connections {
            channels.push(connect(&daemon.socket).await);
        }
        create_stream(&channels[0]).await;
        let (entities, signals) = provision(&channels, shape, mode).await;
        let rate = shape.signals() as f64 / signals.as_secs_f64();
        println!(
            "| {} | {connections} | {} | {} | {rate:.0} | {} |",
            mode.label(),
            secs(entities),
            secs(signals),
            secs(entities + signals),
        );
        drop(channels);
        drop(daemon);
    }
}

fn secs(d: Duration) -> String {
    format!("{:.2} s", d.as_secs_f64())
}
