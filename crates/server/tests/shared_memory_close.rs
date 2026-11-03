//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Closing a sender preserves unread frames until its responses are drained.
#![cfg(unix)]
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "common/mod.rs"]
mod common;
#[path = "shm_common/mod.rs"]
mod shm_common;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{batch, double, stored};
use norn_ipc::{
    AppendCall, AppendOptions, Attach, DataPlane, DataPlaneKind, SendFuture, SharedMemoryTicket,
};
use norn_protocol::rpc::StatusExt;
use norn_protocol::telemetry::AcknowledgementLevel::Buffered;
use norn_shared_memory::SharedMemory;
use shm_common::{call, eventually, on, start};
use tonic::Code;

struct Tracked(Arc<AtomicUsize>);

impl Attach for Tracked {
    fn attach(&self, ticket: &SharedMemoryTicket) -> io::Result<Box<dyn DataPlane>> {
        Ok(Box::new(Publisher {
            inner: SharedMemory.attach(ticket)?,
            dropped: Arc::clone(&self.0),
        }))
    }
}

struct Publisher {
    inner: Box<dyn DataPlane>,
    dropped: Arc<AtomicUsize>,
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl DataPlane for Publisher {
    fn kind(&self) -> DataPlaneKind {
        self.inner.kind()
    }

    fn max_frame(&self) -> usize {
        self.inner.max_frame()
    }

    fn send<'a>(&'a self, frame: &'a [u8]) -> SendFuture<'a> {
        self.inner.send(frame)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_sender_keeps_the_publisher_until_all_replies_arrive() {
    let d = start(&on("")).await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let options = AppendOptions::default().shared_memory(Arc::new(Tracked(Arc::clone(&dropped))));
    let opened = AppendCall::open(d.channel.clone(), &options).await.unwrap();
    assert_eq!(opened.plane(), DataPlaneKind::SharedMemory);
    for id in 1..=2 {
        opened
            .send(batch(id, vec![double("t", id)], Buffered))
            .await
            .unwrap();
    }
    let (sender, mut replies) = opened.into_parts();
    drop(sender);
    assert_eq!(dropped.load(Ordering::SeqCst), 0, "frames still readable");
    let mut ids = Vec::new();
    while let Some(reply) = replies.message().await.unwrap() {
        assert!(reply.error.is_none(), "{reply:?}");
        ids.push(reply.batch_id);
    }
    assert_eq!(ids, [1, 2]);
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "released at end of stream"
    );
    let data = d.stop_keeping().await;
    let sequences: Vec<_> = stored(&data).iter().map(|s| s.sequence.get()).collect();
    assert_eq!(sequences, [1, 2]);
    let _ = std::fs::remove_dir_all(data.parent().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn abandoning_the_replies_releases_the_publisher_and_ring() {
    let d = start(&on("max_rings = 1")).await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let options = AppendOptions::default().shared_memory(Arc::new(Tracked(Arc::clone(&dropped))));
    let opened = AppendCall::open(d.channel.clone(), &options).await.unwrap();
    assert_eq!(opened.plane(), DataPlaneKind::SharedMemory);
    let (sender, replies) = opened.into_parts();
    drop(sender);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(replies);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    eventually("the abandoned ring is granted again", async || {
        call(&d.channel).await.plane() == DataPlaneKind::SharedMemory
    })
    .await;
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_ring_call_reports_shutdown_and_does_not_hold_the_drain() {
    let d = start(&on("")).await;
    let opened = call(&d.channel).await;
    assert_eq!(opened.plane(), DataPlaneKind::SharedMemory);
    opened
        .send(batch(1, vec![double("t", 1)], Buffered))
        .await
        .unwrap();
    let (sender, mut replies) = opened.into_parts();
    let first = replies.message().await.unwrap().unwrap();
    assert_eq!(first.batch_id, 1);
    assert!(first.error.is_none(), "{first:?}");
    d.cancel.cancel();
    let status = tokio::time::timeout(Duration::from_secs(4), replies.message())
        .await
        .expect("an idle ring call held the drain")
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    let info = status.get_details_error_info().unwrap();
    assert_eq!(info.reason, "ERROR_REASON_SHUTTING_DOWN");
    let report = tokio::time::timeout(Duration::from_secs(4), d.task)
        .await
        .expect("the daemon did not stop promptly")
        .unwrap()
        .unwrap();
    assert_eq!(report.aborted, 0);
    drop(sender);
    let _ = std::fs::remove_dir_all(d.data.parent().unwrap());
}
