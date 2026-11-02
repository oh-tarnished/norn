//! Graceful drain, the drain bound, and connection limits at the gRPC level.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{check, start, tcp_endpoint};
use norn_ipc::{Limits, Overflow, ServerConfig, connect};
use tonic_health::pb::health_check_response::ServingStatus;

const SERVING: i32 = ServingStatus::Serving as i32;

#[tokio::test]
async fn an_in_flight_call_completes_during_a_graceful_drain() {
    let config = ServerConfig::default().drain_timeout(Duration::from_secs(5));
    let server = start(&tcp_endpoint(), config, Duration::from_millis(400)).await;
    let channel = connect(&server.endpoint).await.unwrap();
    let call = tokio::spawn(check(channel));
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.shutdown.cancel();

    assert_eq!(call.await.unwrap().unwrap(), SERVING);
    let report = server.task.await.unwrap().unwrap();
    assert_eq!(report.aborted, 0);
}

#[tokio::test]
async fn a_call_outliving_the_drain_bound_is_aborted() {
    let config = ServerConfig::default().drain_timeout(Duration::from_millis(200));
    let server = start(&tcp_endpoint(), config, Duration::from_secs(30)).await;
    let channel = connect(&server.endpoint).await.unwrap();
    let call = tokio::spawn(check(channel));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    server.shutdown.cancel();
    let report = server.task.await.unwrap().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(report.aborted, 1);
    assert!(
        call.await.unwrap().is_err(),
        "aborted call reported success"
    );
}

#[tokio::test]
async fn no_new_client_is_served_once_shutdown_begins() {
    let config = ServerConfig::default().drain_timeout(Duration::from_secs(5));
    let server = start(&tcp_endpoint(), config, Duration::from_millis(500)).await;
    let busy = connect(&server.endpoint).await.unwrap();
    let call = tokio::spawn(check(busy));
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.shutdown.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let late = connect(&server.endpoint).await;
    let refused = match late {
        Err(_) => true,
        Ok(channel) => check(channel).await.is_err(),
    };
    assert!(refused, "a client was admitted during the drain");
    assert_eq!(call.await.unwrap().unwrap(), SERVING);
    server.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_client_beyond_the_connection_limit_is_refused() {
    let limits = Limits::default()
        .max_connections(1)
        .overflow(Overflow::Refuse);
    let config = ServerConfig::default().limits(limits);
    let server = start(&tcp_endpoint(), config, Duration::ZERO).await;
    let first = connect(&server.endpoint).await.unwrap();
    assert_eq!(check(first.clone()).await.unwrap(), SERVING);

    let second = match connect(&server.endpoint).await {
        Err(_) => true,
        Ok(channel) => check(channel).await.is_err(),
    };
    assert!(second, "a client beyond the limit was served");
    assert_eq!(check(first).await.unwrap(), SERVING);
    server.shutdown.cancel();
    server.task.await.unwrap().unwrap();
}
