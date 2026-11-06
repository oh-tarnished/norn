//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An ingest row on its own fresh daemon, and the metrics it records. A fresh
//! daemon per row, so no row inherits another's memory, segments or journal.

use std::path::Path;
use std::sync::Arc;

use norn_ipc::{AppendOptions, DataPlaneKind};
use norn_shared_memory::SharedMemory;

use crate::daemon::Daemon;
use crate::ingest::drive;
use crate::profile::{IngestRow, Plane};
use crate::seed::seed;
use crate::setup::watch_rss;
use crate::stats::{Better, Metrics, percentile};

/// Runs `row` once and records `<group>.<case>.*` into `metrics`.
pub async fn run_row(base: &Path, row: &IngestRow, metrics: &mut Metrics) {
    let tag = row.case.replace('.', "-");
    let (daemon, _) = Daemon::start(base, &tag).await;
    let names = seed(&daemon.channel, row.signals).await;
    let options = match row.plane {
        Plane::Socket => AppendOptions::default(),
        Plane::SharedMemory => AppendOptions::default().shared_memory(Arc::new(SharedMemory)),
    };
    let (rss, stop) = watch_rss(daemon.pid());
    let outcome = drive(&daemon.channel, &options, &names, row).await;
    stop.cancel();
    let peak_kib = rss.await.unwrap();
    drop(daemon);
    let key = |measure: &str| format!("{}.{}.{measure}", row.group, row.case);
    let mut put = |measure: &str, unit: &str, better: Better, gated: bool, value: f64| {
        metrics.record(&key(measure), unit, better, gated, value);
    };
    put(
        "achieved_per_s",
        "samples/s",
        Better::Higher,
        true,
        outcome.achieved,
    );
    let at = |q| percentile(&outcome.acks_ms, q).unwrap_or(f64::NAN);
    put("ack_p50_ms", "ms", Better::Lower, true, at(0.50));
    put("ack_p99_ms", "ms", Better::Lower, false, at(0.99));
    put(
        "throttled_batches",
        "batches",
        Better::Lower,
        false,
        outcome.throttled as f64,
    );
    put(
        "rejected_batches",
        "batches",
        Better::Lower,
        false,
        outcome.rejected as f64,
    );
    put(
        "peak_rss_mib",
        "MiB",
        Better::Lower,
        false,
        peak_kib as f64 / 1024.0,
    );
    if row.plane == Plane::SharedMemory {
        // A ring may be refused; the row then ran on the socket, and says so.
        let granted = outcome.plane == DataPlaneKind::SharedMemory;
        put(
            "ring_granted",
            "0 or 1",
            Better::Higher,
            false,
            f64::from(u8::from(granted)),
        );
    }
    println!(
        "{}.{}: offered {}/s, achieved {:.0}/s, ack p50 {:.2} ms, p99 {:.2} ms, plane {:?}",
        row.group,
        row.case,
        row.rate,
        outcome.achieved,
        at(0.50),
        at(0.99),
        outcome.plane
    );
}
