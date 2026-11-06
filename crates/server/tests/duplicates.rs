//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A resend after a restart is a duplicate even when its original was late data,
//! written after the signal's latest sample: the window a restart rebuilds holds
//! what was written last, not what is latest in time (docs/reference/storage.md
//! "Duplicates: exactly what is guaranteed").
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod process;
mod query_common;

use std::path::PathBuf;

use common::{append, batch, seed, start, temp};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_server::DaemonConfig;
use process::{connect, spawn};
use query_common::{all, samples};

fn at(seconds: i64, sequence: i64, value: f64) -> Sample {
    Sample {
        signal: "entities/dev/signals/t".into(),
        observe_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(value)),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence,
    }
}

/// One stored sample at 1053 s, then, after a restart, a late one at 1017 s.
async fn late_after_latest(tag: &str) -> PathBuf {
    let d = start(DaemonConfig::default(), temp(tag)).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    append(&d.channel, vec![batch(1, vec![at(1_053, 0, 0.0)], Durable)]).await;
    let d = start(DaemonConfig::default(), d.stop_keeping().await).await;
    append(
        &d.channel,
        vec![batch(2, vec![at(1_017, 26, 1.0)], Durable)],
    )
    .await;
    d.stop_keeping().await
}

/// Resends the late sample with another value: it must be counted, not stored,
/// and the original must be what a query returns among the `stored` samples.
async fn resend_is_a_duplicate(data: PathBuf, stored: usize) {
    let d = start(DaemonConfig::default(), data).await;
    let replies = append(
        &d.channel,
        vec![batch(3, vec![at(1_017, 26, 2.0)], Durable)],
    )
    .await;
    assert!(
        replies.iter().all(|r| r.duplicate_count == 1),
        "{replies:?}"
    );
    let got = samples(&d.channel, all()).await;
    assert_eq!(got.len(), stored, "{got:?}");
    let late = got.iter().find(|s| s.sequence == 26).unwrap();
    assert_eq!(
        late.value.as_ref().unwrap().kind,
        Some(Kind::DoubleValue(1.0))
    );
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resend_of_late_data_after_a_clean_restart_is_a_duplicate() {
    resend_is_a_duplicate(late_after_latest("dup-clean").await, 2).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resend_of_late_data_after_a_crash_is_a_duplicate() {
    let data = late_after_latest("dup-crash").await;
    let root = temp("dup-crash-ipc");
    std::fs::create_dir_all(&root).unwrap();
    let socket = root.join("ipc/n.sock");
    // A run that ends in kill -9: the next start recovers, with no clean marker.
    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    append(&channel, vec![batch(4, vec![at(1_054, 1, 3.0)], Durable)]).await;
    drop(channel);
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    resend_is_a_duplicate(data, 3).await;
    let _ = std::fs::remove_dir_all(&root);
}
