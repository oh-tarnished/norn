//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! [`Sink`](norn_sink::Sink) against a fake daemon over a real Unix socket:
//! everything the daemon might say, and the daemon not being there at all.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

mod api;
mod daemon;
mod fake;
mod outage;
mod provisioning;
mod registry;
mod replies;
mod scale;
mod stubs;
mod support;

use daemon::Daemon;
use norn_protocol::telemetry::{BackpressureOutcome as O, BackpressureReason as R};
use std::time::Duration;
use support::{Socket, acknowledged, holding, readings, scripted};

#[tokio::test]
async fn throttle_stores_the_batch_and_holds_the_next_for_its_delay() {
    let socket = Socket::new("throttle");
    let log = scripted(&[(O::Throttle, R::BufferHighWater, Some(300))]);
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = holding(socket.endpoint(), 1 << 20);
    run.sink.push(readings(0, 2)).unwrap();
    acknowledged(&run.sink, 2).await;
    run.sink.push(readings(2, 2)).unwrap();
    acknowledged(&run.sink, 4).await;
    {
        let seen = log.lock().unwrap();
        assert_eq!(seen.arrivals.len(), 2, "one batch each");
        let gap = seen.arrivals[1] - seen.arrivals[0];
        assert!(gap >= Duration::from_millis(290), "{gap:?}");
        assert_eq!(seen.arrived.len(), 4, "not one resent");
    }
    assert_eq!(run.sink.counts().throttled, 1);
    run.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn buffer_is_only_a_warning_and_nothing_slows() {
    let socket = Socket::new("buffer");
    let log = scripted(&[(O::Buffer, R::WritePathBehind, None)]);
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = holding(socket.endpoint(), 1 << 20);
    run.sink.push(readings(0, 4)).unwrap();
    acknowledged(&run.sink, 4).await;
    assert_eq!(log.lock().unwrap().arrived.len(), 4);
    run.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn a_transient_reject_resends_the_batch_after_its_delay() {
    let socket = Socket::new("reject");
    let log = scripted(&[(O::Reject, R::BufferFull, Some(200))]);
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = holding(socket.endpoint(), 1 << 20);
    run.sink.push(readings(0, 2)).unwrap();
    acknowledged(&run.sink, 2).await;
    {
        let seen = log.lock().unwrap();
        assert_eq!(seen.stored, [1, 2], "the same samples, once");
        assert!(seen.arrivals[1] - seen.arrivals[0] >= Duration::from_millis(190));
    }
    assert_eq!(run.sink.counts().rejected, 1);
    run.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn a_full_buffer_drops_the_oldest_says_how_many_and_turns_degraded() {
    let socket = Socket::new("full");
    // Bytes, not readings: one byte holds none of the five.
    let run = holding(socket.endpoint(), 1);
    let pushed = run.sink.push(readings(0, 5)).unwrap();
    assert_eq!(pushed.dropped, 5);
    assert_eq!(run.sink.counts().overflowed, 5);
    let health = run.sink.health();
    assert!(health.degraded, "a loss is loud");
    assert_eq!(health.dropped, 5);
    let report = run.close().await;
    assert_eq!(report.counts.acknowledged, 0, "there was never a Norn");
}
