//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The shared-memory data plane against a real daemon: round trips, and the
//! socket fallback whenever a ring is not granted.
// Rings are never offered on Windows (docs/reference/shared-memory.md).
#![cfg(unix)]
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "common/mod.rs"]
mod common;
#[path = "shm_common/mod.rs"]
mod shm_common;

use std::collections::BTreeSet;
use std::io;
use std::sync::Arc;

use common::{batch, double, seed, stored, temp};
use norn_ipc::{AppendCall, AppendOptions, Attach, DataPlane, DataPlaneKind, SharedMemoryTicket};
use norn_protocol::common::ValueType;
use norn_protocol::telemetry::AcknowledgementLevel::{Buffered, Durable};
use norn_server::DaemonConfig;
use shm_common::{call, finish, offered, on, start};

#[tokio::test(flavor = "multi_thread")]
async fn batches_round_trip_through_a_ring_with_durable_acks_on_the_socket() {
    let d = start(&on("")).await;
    assert!(offered(&d.channel).await);
    let call = call(&d.channel).await;
    assert_eq!(call.plane(), DataPlaneKind::SharedMemory);
    for id in 1..=20 {
        let from = (id - 1) * 10;
        let samples = (from..from + 10).map(|n| double("t", n)).collect();
        call.send(batch(id, samples, Durable)).await.unwrap();
    }
    let replies = finish(call).await;
    assert!(replies.iter().all(|r| r.error.is_none()), "{replies:?}");
    let durable: Vec<_> = replies
        .iter()
        .filter(|r| r.ack_level == Durable as i32)
        .map(|r| r.batch_id)
        .collect();
    assert_eq!(durable, (1..=20).collect::<Vec<_>>());
    let data = d.stop_keeping().await;
    let sequences: Vec<u64> = stored(&data).iter().map(|s| s.sequence.get()).collect();
    let unique: BTreeSet<_> = sequences.iter().copied().collect();
    assert_eq!(sequences.len(), 200, "every sample, once");
    assert_eq!(unique, (0..200).collect());
    let _ = std::fs::remove_dir_all(data.parent().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_with_shared_memory_off_keeps_the_client_on_the_socket() {
    let d = start("").await;
    assert!(!offered(&d.channel).await);
    let call = call(&d.channel).await;
    assert_eq!(call.plane(), DataPlaneKind::ControlPlane);
    call.send(batch(1, vec![double("t", 0)], Buffered))
        .await
        .unwrap();
    let replies = finish(call).await;
    assert_eq!(replies[0].ack_level, Buffered as i32);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_memory_on_a_tcp_endpoint_is_unavailable_and_appends_still_work() {
    let config = DaemonConfig::from_toml(&on("")).unwrap();
    let d = common::start(config, temp("shm-tcp")).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    assert!(
        !offered(&d.channel).await,
        "only a Unix socket proves the user"
    );
    let call = call(&d.channel).await;
    assert_eq!(call.plane(), DataPlaneKind::ControlPlane);
    call.send(batch(1, vec![double("t", 0)], Buffered))
        .await
        .unwrap();
    assert!(finish(call).await[0].error.is_none());
    d.stop().await;
}

/// A client whose attach always fails, as across a container boundary.
struct Unreachable;

impl Attach for Unreachable {
    fn attach(&self, _: &SharedMemoryTicket) -> io::Result<Box<dyn DataPlane>> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_granted_ring_the_client_cannot_reach_leaves_the_call_on_the_socket() {
    let d = start(&on("")).await;
    let options = AppendOptions::default().shared_memory(Arc::new(Unreachable));
    let call = AppendCall::open(d.channel.clone(), &options).await.unwrap();
    assert_eq!(call.plane(), DataPlaneKind::ControlPlane);
    for id in 1..=3 {
        call.send(batch(id, vec![double("t", id)], Durable))
            .await
            .unwrap();
    }
    let replies = finish(call).await;
    let durable = replies.iter().filter(|r| r.ack_level == Durable as i32);
    assert_eq!(durable.count(), 3, "{replies:?}");
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_past_max_rings_stays_on_the_socket_and_a_freed_ring_is_reused() {
    let d = start(&on("max_rings = 1")).await;
    let first = call(&d.channel).await;
    let second = call(&d.channel).await;
    assert_eq!(first.plane(), DataPlaneKind::SharedMemory);
    assert_eq!(second.plane(), DataPlaneKind::ControlPlane);
    second
        .send(batch(1, vec![double("t", 0)], Buffered))
        .await
        .unwrap();
    assert_eq!(finish(second).await.len(), 1);
    finish(first).await;
    let channel = d.channel.clone();
    shm_common::eventually("the ring is granted again", async || {
        call(&channel).await.plane() == DataPlaneKind::SharedMemory
    })
    .await;
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_too_big_for_a_frame_goes_by_the_socket_in_its_place() {
    let d = start(&on("max_frame_bytes = 4096")).await;
    let call = call(&d.channel).await;
    assert_eq!(call.plane(), DataPlaneKind::SharedMemory);
    call.send(batch(1, vec![double("t", 0)], Buffered))
        .await
        .unwrap();
    let big = (1..400).map(|n| double("t", n)).collect();
    call.send(batch(2, big, Buffered)).await.unwrap();
    assert_eq!(call.plane(), DataPlaneKind::ControlPlane);
    call.send(batch(3, vec![double("t", 400)], Buffered))
        .await
        .unwrap();
    let ids: Vec<_> = finish(call).await.iter().map(|r| r.batch_id).collect();
    assert_eq!(ids, [1, 2, 3]);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_one_slot_ring_under_a_flood_answers_every_batch_in_order() {
    let d = start(&on("slot_count = 1\nmax_frame_bytes = 65536")).await;
    let call = call(&d.channel).await;
    assert_eq!(call.plane(), DataPlaneKind::SharedMemory);
    let (sender, mut replies) = call.into_parts();
    let batches = 2_000;
    let writer = tokio::spawn(async move {
        for id in 1..=batches {
            let from = (id - 1) * 50;
            let samples = (from..from + 50).map(|n| double("t", n)).collect();
            sender.send(batch(id, samples, Buffered)).await.unwrap();
        }
    });
    let mut answered = 0;
    while let Some(r) = replies.message().await.unwrap() {
        answered += 1;
        assert_eq!(r.batch_id, answered, "in order, none lost");
    }
    writer.await.unwrap();
    assert_eq!(answered, batches);
    d.stop().await;
}
