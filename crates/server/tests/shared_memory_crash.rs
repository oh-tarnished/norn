//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A writer that crashes, mid-frame and mid-flood: the daemon admits only whole
//! batches, each once, frees the ring, and keeps serving.
//!
//! The writers are this same test binary, run as a child with `NORN_SHM_CHILD`
//! set to the daemon's socket: a process that is really killed is the only
//! honest test of a crash.
// Rings are never offered on Windows (docs/reference/shared-memory.md).
#![cfg(unix)]
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "common/mod.rs"]
mod common;
#[path = "shm_common/mod.rs"]
mod shm_common;
#[path = "shm_crash/mod.rs"]
mod shm_crash;

use std::collections::BTreeMap;
use std::time::Duration;

use common::{batch, double, stored};
use norn_ipc::DataPlaneKind;
use norn_protocol::telemetry::AcknowledgementLevel::Buffered;
use shm_common::{call, eventually, finish, on, start};
use shm_crash::{child, child_channel, hostile, target, ticket, wait_for};

/// A sequence beyond any child's, for checking the daemon still serves.
const SERVING_CHECK: i64 = 1 << 30;

async fn still_serves(channel: &tonic::transport::Channel) {
    let call = call(channel).await;
    call.send(batch(1, vec![double("t", SERVING_CHECK)], Buffered))
        .await
        .unwrap();
    let replies = finish(call).await;
    assert!(replies[0].error.is_none(), "{replies:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_writer_killed_mid_frame_publishes_nothing_and_frees_its_ring() {
    const NAME: &str = "a_writer_killed_mid_frame_publishes_nothing_and_frees_its_ring";
    if let Some(socket) = target() {
        let (ticket, _call) = ticket(child_channel(socket).await).await;
        let peer = hostile(&ticket);
        let mut sample = peer.publisher.loan_slice_uninit(1024).unwrap();
        for byte in sample.payload_mut().iter_mut().take(512) {
            byte.write(0xAB);
        }
        println!("WRITING");
        std::thread::sleep(Duration::from_secs(600));
        return;
    }
    let d = start(&on("max_rings = 1")).await;
    let mut writer = child(&d, NAME);
    wait_for(&mut writer, "WRITING");
    let held = call(&d.channel).await;
    assert_eq!(
        held.plane(),
        DataPlaneKind::ControlPlane,
        "the child holds the ring"
    );
    drop(held);
    writer.kill().unwrap();
    writer.wait().unwrap();
    let channel = d.channel.clone();
    eventually("the dead writer's ring is granted again", async || {
        call(&channel).await.plane() == DataPlaneKind::SharedMemory
    })
    .await;
    still_serves(&d.channel).await;
    let data = d.stop_keeping().await;
    let kept: Vec<u64> = stored(&data).iter().map(|s| s.sequence.get()).collect();
    assert_eq!(
        kept,
        [SERVING_CHECK as u64],
        "nothing of the half-written frame"
    );
    let _ = std::fs::remove_dir_all(data.parent().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_writer_killed_in_a_flood_leaves_only_whole_batches_each_once() {
    const NAME: &str = "a_writer_killed_in_a_flood_leaves_only_whole_batches_each_once";
    if let Some(socket) = target() {
        let call = call(&child_channel(socket).await).await;
        assert_eq!(call.plane(), DataPlaneKind::SharedMemory);
        let (sender, mut replies) = call.into_parts();
        tokio::spawn(async move { while replies.message().await.is_ok_and(|r| r.is_some()) {} });
        println!("FLOODING");
        for id in 1.. {
            let from = (id - 1) * 100;
            let samples = (from..from + 100).map(|n| double("t", n)).collect();
            if sender.send(batch(id, samples, Buffered)).await.is_err() {
                return;
            }
        }
        return;
    }
    let d = start(&on("")).await;
    let mut writer = child(&d, NAME);
    wait_for(&mut writer, "FLOODING");
    tokio::time::sleep(Duration::from_millis(300)).await;
    writer.kill().unwrap();
    writer.wait().unwrap();
    still_serves(&d.channel).await;
    let data = d.stop_keeping().await;
    let mut batches = BTreeMap::<u64, u32>::new();
    let mut seen = std::collections::BTreeSet::new();
    for sample in stored(&data) {
        let n = sample.sequence.get();
        assert!(seen.insert(n), "sequence {n} stored twice");
        if n < SERVING_CHECK as u64 {
            *batches.entry(n / 100).or_default() += 1;
        }
    }
    assert!(
        !batches.is_empty(),
        "the flood stored nothing, so this proved nothing"
    );
    let cut: Vec<_> = batches.iter().filter(|(_, n)| **n != 100).collect();
    assert!(cut.is_empty(), "batches cut short: {cut:?}");
    let _ = std::fs::remove_dir_all(data.parent().unwrap());
}
