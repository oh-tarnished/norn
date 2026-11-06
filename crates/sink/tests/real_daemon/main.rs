//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! [`Sink`] against a real daemon, in-process on a temporary socket and data
//! directory: what a fake can only claim. Its own 256 KiB limit, what it stores
//! across a restart, a restart of the adapter, and, with the `shared-memory`
//! feature, a ring it grants.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

mod support;

use norn_protocol::signal::GetSignalRequest;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_sink::{Notice, Options, Sink};
use std::sync::{Arc, Mutex};
use support::{Dir, catalog, sample_count, spawned, start, sweep, within};

#[tokio::test(flavor = "multi_thread")]
async fn a_large_catalog_lands_inside_the_real_limit_and_every_reading_is_stored() {
    let dir = Dir::new("large");
    let daemon = start(&dir, "").await;
    let refused = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&refused);
    let options = Options::default().on_notice(move |n| {
        if matches!(n, Notice::ResourceRefused { .. }) {
            keep.lock().unwrap().push(n.clone());
        }
    });
    let sink = Sink::new(dir.endpoint(), "plant", options).unwrap();
    let run = spawned(&sink);
    sink.declare(&catalog("rpm", 3_000)).unwrap();
    sink.push(sweep(1, 3_000)).unwrap();
    within("settling", sink.settled()).await;
    assert_eq!(sample_count(&dir.endpoint()).await, 3_000);
    sink.close().await;
    let report = run.await.unwrap().unwrap();
    assert_eq!(report.counts.acknowledged, 3_000);
    assert_eq!(report.counts.invalid, 0);
    assert!(refused.lock().unwrap().is_empty(), "{refused:?}");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_restart_mid_stream_loses_nothing_and_stores_nothing_twice() {
    let dir = Dir::new("restart");
    let daemon = start(&dir, "").await;
    let sink = Sink::new(dir.endpoint(), "plant", Options::default()).unwrap();
    let run = spawned(&sink);
    sink.declare(&catalog("rpm", 50)).unwrap();
    sink.push(sweep(1, 50)).unwrap();
    within("the first sweep", sink.settled()).await;
    // Readings pushed around the stop: some answered, some not, none lost.
    sink.push(sweep(2, 50)).unwrap();
    daemon.stop().await;
    sink.push(sweep(3, 50)).unwrap();
    assert!(sink.health().held > 0, "held while Norn is away");
    let daemon = start(&dir, "").await;
    sink.push(sweep(4, 50)).unwrap();
    within("the rest", sink.settled()).await;
    assert_eq!(
        sample_count(&dir.endpoint()).await,
        200,
        "each exactly once"
    );
    sink.close().await;
    let report = run.await.unwrap().unwrap();
    assert_eq!(report.counts.acknowledged, 200);
    assert_eq!(report.counts.dropped(), 0);
    assert_eq!(report.never_sent, 0);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adapter_restart_creates_nothing_anew_and_updates_only_what_changed() {
    let dir = Dir::new("again");
    let daemon = start(&dir, "").await;
    for (unit, round) in [("rpm", 1), ("rpm", 2), ("hertz", 3)] {
        let sink = Sink::new(dir.endpoint(), "plant", Options::default()).unwrap();
        let run = spawned(&sink);
        sink.declare(&catalog(unit, 10)).unwrap();
        sink.push(sweep(round, 10)).unwrap();
        within("a run", sink.settled()).await;
        sink.close().await;
        let report = run.await.unwrap().unwrap();
        assert_eq!(report.counts.acknowledged, 10);
    }
    assert_eq!(sample_count(&dir.endpoint()).await, 30);
    let channel = norn_ipc::connect(&dir.endpoint()).await.unwrap();
    let name = "entities/rig/signals/s7".to_owned();
    let signal = SignalServiceClient::new(channel)
        .get_signal(GetSignalRequest { name })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(signal.unit, "hertz");
    daemon.stop().await;
}

#[cfg(feature = "shared-memory")]
#[tokio::test(flavor = "multi_thread")]
async fn a_ring_the_daemon_grants_carries_the_batches_and_says_so() {
    let dir = Dir::new("ring");
    let daemon = start(&dir, "[shared_memory]\nenabled = true\n").await;
    let told = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&told);
    let options = Options::default()
        .shared_memory(true)
        .on_notice(move |n| keep.lock().unwrap().push(n.clone()));
    let sink = Sink::new(dir.endpoint(), "plant", options).unwrap();
    let run = spawned(&sink);
    sink.declare(&catalog("rpm", 20)).unwrap();
    sink.push(sweep(1, 20)).unwrap();
    within("the ring", sink.settled()).await;
    assert_eq!(sample_count(&dir.endpoint()).await, 20);
    sink.close().await;
    run.await.unwrap().unwrap();
    let granted = Notice::Plane {
        shared_memory: true,
    };
    assert!(told.lock().unwrap().contains(&granted), "{told:?}");
    daemon.stop().await;
}
