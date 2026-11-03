//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Stopping with clients still connected: an idle connection, and an
//! `AppendSamples` call held open between batches the way a long-running
//! collector holds it, must not keep the graceful drain waiting for its bound.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{Requests, batch, double, seed, start, temp};
use norn_protocol::common::ValueType::Double;
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_server::DaemonConfig;
use tokio::sync::mpsc;
use tonic::Code;

/// Ten seconds, the default: a stop that took it would be the bug.
const DRAIN: Duration = Duration::from_secs(10);
/// "Well under": what a stop with nothing in flight may take, with slack for a
/// loaded machine.
const PROMPT: Duration = Duration::from_secs(4);

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_with_an_idle_client_stops_well_under_the_drain_timeout() {
    let config = DaemonConfig::default().drain_timeout(DRAIN);
    let d = start(config, temp("drain-idle")).await;
    seed(&d.channel, &[("t", Double)]).await;
    // `d.channel` stays connected, idle, through the stop.
    let started = Instant::now();
    let data = d.stop_keeping().await;
    let took = started.elapsed();
    assert!(
        took < PROMPT,
        "an idle connection held the drain for {took:?}"
    );
    let _ = std::fs::remove_dir_all(data);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_append_call_ends_unavailable_at_shutdown_instead_of_holding_the_drain() {
    let config = DaemonConfig::default().drain_timeout(DRAIN);
    let d = start(config, temp("drain-append")).await;
    seed(&d.channel, &[("t", Double)]).await;
    let (tx, rx) = mpsc::channel(4);
    let mut client = TelemetryServiceClient::new(d.channel.clone());
    let mut replies = client
        .append_samples(Requests(rx))
        .await
        .unwrap()
        .into_inner();
    tx.send(batch(1, vec![double("t", 1)], Persisted))
        .await
        .unwrap();
    // The first answer, then the second once persisted: nothing in flight.
    for _ in 0..2 {
        let reply = replies.message().await.unwrap().unwrap();
        assert!(reply.error.is_none(), "{reply:?}");
    }
    // `tx` stays alive: the call is open and idle, as a collector keeps it.
    let started = Instant::now();
    d.cancel.cancel();
    let ended = tokio::time::timeout(DRAIN, replies.message()).await;
    let status = match ended.expect("the call outlived the drain bound") {
        Err(status) => status,
        Ok(other) => panic!("expected UNAVAILABLE, got {other:?}"),
    };
    assert_eq!(status.code(), Code::Unavailable, "{status:?}");
    d.task.await.unwrap().unwrap();
    let took = started.elapsed();
    assert!(took < PROMPT, "an idle append held the drain for {took:?}");
    drop(tx);
    let _ = std::fs::remove_dir_all(&d.data);
}
