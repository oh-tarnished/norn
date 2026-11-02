//! A replay session outlives no client: leaving, or stopping the server, stops it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::time::{Duration, Instant};

use common::seed::{TEMP, seed};
use common::{Running, stdio, temp};
use norn_protocol::replay::ListReplaySessionsRequest;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use serde_json::json;
use tonic::transport::Channel;

async fn open_sessions(channel: &Channel) -> usize {
    let list = ListReplaySessionsRequest::default();
    let reply = ReplayServiceClient::new(channel.clone())
        .list_replay_sessions(list)
        .await;
    reply.unwrap().into_inner().replay_sessions.len()
}

async fn until_only(channel: &Channel, left: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while open_sessions(channel).await != left {
        assert!(Instant::now() < deadline, "sessions were left behind");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_leaving_stdio_stops_its_sessions() {
    let daemon = Running::start(temp("leave")).await;
    let channel = daemon.channel().await;
    seed(&channel).await;
    let mut c = stdio(&daemon.endpoint).await;
    let start = json!({"action": "start", "target": TEMP, "from": "1000", "to": "1010"});
    c.ok("replay", start).await;
    assert_eq!(open_sessions(&channel).await, 1);
    c.service.cancel().await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), c.child.wait()).await;
    assert!(
        status.unwrap().unwrap().success(),
        "norn-mcp must exit cleanly"
    );
    until_only(&channel, 0).await;
    daemon.finish().await;
}

/// Unix only: it sends `norn-mcp` SIGTERM. Windows has no SIGTERM, and a test
/// cannot send a child Ctrl-C without sharing its console; stopping by Ctrl-C is
/// the same `terminated` path in `services/mcp/src/run.rs`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_http_server_stops_its_sessions_too() {
    let daemon = Running::start(temp("term")).await;
    let channel = daemon.channel().await;
    seed(&channel).await;
    let (child, address) = common::http_server(&daemon.endpoint, &[]).await;
    let uri = format!("http://{address}/mcp");
    let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(uri);
    let service = rmcp::ServiceExt::serve((), transport).await.unwrap();
    let c = common::Client {
        service,
        child,
        address: Some(address),
    };
    let start = json!({"action": "start", "target": TEMP, "from": "1000", "to": "1010"});
    c.ok("replay", start).await;
    let pid = c.child.id().unwrap().to_string();
    let term = std::process::Command::new("kill")
        .args(["-TERM", &pid])
        .status();
    assert!(term.unwrap().success());
    until_only(&channel, 0).await;
    daemon.finish().await;
}
