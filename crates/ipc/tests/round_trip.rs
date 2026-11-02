//! A gRPC call round-trips over every transport this OS offers, and the
//! client never names the transport.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{check, start, tcp_endpoint, unix_dir, unix_endpoint};
use norn_ipc::{Endpoint, IpcError, ServerConfig, connect, connect_lazy};
use tonic_health::pb::health_check_response::ServingStatus;

const SERVING: i32 = ServingStatus::Serving as i32;

async fn round_trip(endpoint: Endpoint) {
    let server = start(&endpoint, ServerConfig::default(), Duration::ZERO).await;
    let channel = connect(&server.endpoint).await.unwrap();
    for _ in 0..3 {
        assert_eq!(check(channel.clone()).await.unwrap(), SERVING);
    }
    drop(channel);
    server.shutdown.cancel();
    let report = server.task.await.unwrap().unwrap();
    assert_eq!(report.aborted, 0);
}

#[cfg(unix)]
#[tokio::test]
async fn a_call_round_trips_over_a_unix_socket() {
    let endpoint = unix_endpoint();
    round_trip(endpoint.clone()).await;
    let dir = unix_dir(&endpoint).unwrap();
    assert!(!dir.join("n.sock").exists(), "socket left behind");
    std::fs::remove_dir(dir).unwrap();
}

#[tokio::test]
async fn a_call_round_trips_over_loopback_tcp() {
    round_trip(tcp_endpoint()).await;
}

#[tokio::test]
async fn a_lazy_channel_connects_on_first_call() {
    let server = start(&tcp_endpoint(), ServerConfig::default(), Duration::ZERO).await;
    let channel = connect_lazy(&server.endpoint);
    assert_eq!(check(channel).await.unwrap(), SERVING);
    server.shutdown.cancel();
    server.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn connecting_to_a_missing_daemon_fails_at_connect() {
    let server = start(&tcp_endpoint(), ServerConfig::default(), Duration::ZERO).await;
    let gone = server.endpoint.clone();
    server.shutdown.cancel();
    server.task.await.unwrap().unwrap();
    let err = connect(&gone).await.err().unwrap();
    assert!(matches!(err, IpcError::Grpc(_)), "{err:?}");
}

#[tokio::test]
async fn a_second_server_on_a_live_socket_is_refused() {
    let endpoint = if cfg!(unix) {
        unix_endpoint()
    } else {
        tcp_endpoint()
    };
    let first = start(&endpoint, ServerConfig::default(), Duration::ZERO).await;
    let second = norn_ipc::Server::bind(&first.endpoint, ServerConfig::default()).await;
    assert!(matches!(second, Err(IpcError::Transport(_))));
    first.shutdown.cancel();
    first.task.await.unwrap().unwrap();
    if let Some(dir) = unix_dir(&endpoint) {
        std::fs::remove_dir(dir).unwrap();
    }
}
