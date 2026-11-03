//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Connection limits, backpressure, idle timeouts and abort, over loopback
//! TCP so they run identically on every OS.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::ErrorKind;
use std::time::Duration;

use norn_transport::{Endpoint, Incoming, Limits, Overflow, TransportError, connect};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

const SHORT: Duration = Duration::from_millis(200);
const LONG: Duration = Duration::from_secs(5);

async fn incoming(limits: Limits) -> (Incoming, Endpoint) {
    let ep: Endpoint = "tcp:127.0.0.1:0".parse().unwrap();
    let incoming = Incoming::bind(&ep, limits).await.unwrap();
    let bound = incoming.local_endpoint().clone();
    (incoming, bound)
}

#[tokio::test]
async fn a_connection_beyond_the_limit_is_refused() {
    let limits = Limits::default()
        .max_connections(1)
        .overflow(Overflow::Refuse);
    let (mut incoming, ep) = incoming(limits).await;
    let _first = connect(&ep).await.unwrap();
    let held = incoming.accept().await.unwrap();

    let mut second = connect(&ep).await.unwrap();
    assert!(timeout(SHORT, incoming.accept()).await.is_err());
    assert_eq!(incoming.refused(), 1);
    let mut byte = [0u8; 1];
    let read = timeout(LONG, second.read(&mut byte)).await.unwrap();
    assert!(
        matches!(read, Ok(0) | Err(_)),
        "refused client saw {read:?}"
    );

    drop(held);
    let _third = connect(&ep).await.unwrap();
    timeout(LONG, incoming.accept()).await.unwrap().unwrap();
}

#[tokio::test]
async fn a_queued_connection_is_admitted_when_a_slot_frees() {
    let limits = Limits::default()
        .max_connections(1)
        .overflow(Overflow::Queue);
    let (mut incoming, ep) = incoming(limits).await;
    let _first = connect(&ep).await.unwrap();
    let held = incoming.accept().await.unwrap();
    assert_eq!(incoming.tracker().active(), 1);

    let mut second = connect(&ep).await.unwrap();
    assert!(timeout(SHORT, incoming.accept()).await.is_err());
    drop(held);
    let mut admitted = timeout(LONG, incoming.accept()).await.unwrap().unwrap();
    second.write_all(b"hi").await.unwrap();
    let mut buf = [0u8; 2];
    admitted.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hi");
}

#[tokio::test]
async fn an_idle_connection_times_out() {
    let limits = Limits::default().idle_timeout(Some(Duration::from_millis(100)));
    let (mut incoming, ep) = incoming(limits).await;
    let _client = connect(&ep).await.unwrap();
    let mut conn = incoming.accept().await.unwrap();
    let mut byte = [0u8; 1];
    let err = timeout(LONG, conn.read(&mut byte))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TimedOut);
}

#[tokio::test]
async fn traffic_keeps_a_connection_alive_past_the_idle_timeout() {
    let limits = Limits::default().idle_timeout(Some(Duration::from_millis(150)));
    let (mut incoming, ep) = incoming(limits).await;
    let mut client = connect(&ep).await.unwrap();
    let mut conn = incoming.accept().await.unwrap();
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(80)).await;
        client.write_all(b"x").await.unwrap();
        let mut byte = [0u8; 1];
        conn.read_exact(&mut byte).await.unwrap();
    }
}

#[tokio::test]
async fn an_aborted_connection_fails_its_pending_read() {
    let (mut incoming, ep) = incoming(Limits::default()).await;
    let _client = connect(&ep).await.unwrap();
    let mut conn = incoming.accept().await.unwrap();
    let abort = incoming.abort_handle();
    let reader = tokio::spawn(async move {
        let mut byte = [0u8; 1];
        conn.read(&mut byte).await
    });
    tokio::time::sleep(SHORT).await;
    abort.cancel();
    let err = timeout(LONG, reader).await.unwrap().unwrap().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ConnectionAborted);
}

#[tokio::test]
async fn the_tracker_resolves_once_every_connection_is_dropped() {
    let (mut incoming, ep) = incoming(Limits::default().max_connections(4)).await;
    let tracker = incoming.tracker();
    let _client = connect(&ep).await.unwrap();
    let conn = incoming.accept().await.unwrap();
    drop(incoming);
    assert!(timeout(SHORT, tracker.closed()).await.is_err());
    drop(conn);
    timeout(LONG, tracker.closed()).await.unwrap();
    assert_eq!(tracker.active(), 0);
}

#[tokio::test]
async fn zero_connections_is_an_invalid_limit() {
    let ep: Endpoint = "tcp:127.0.0.1:0".parse().unwrap();
    let err = Incoming::bind(&ep, Limits::default().max_connections(0)).await;
    assert!(matches!(err, Err(TransportError::InvalidLimits(_))));
}
