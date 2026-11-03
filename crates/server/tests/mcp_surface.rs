//! MCP's surface: read-only by listing it, off by default, no TCP port unless one is
//! named, and sessions out of reach. Against the real `norn` binary.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "mcp_common/mod.rs"]
mod mcp_common;

use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

use mcp_common::{Norn, mcp_socket, temp};
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{CreateReplaySessionRequest, ReplaySession};
use rmcp::model::CallToolRequestParams;
use serde_json::json;

const TOOLS: [&str; 17] = [
    "admin_service-get_server_v1",
    "entity_service-get_entity_v1",
    "entity_service-list_entities_v1",
    "query_service-browse_samples_v1",
    "query_service-compare_periods_v1",
    "query_service-compute_stats_v1",
    "query_service-find_events_v1",
    "replay_service-create_replay_session_v1",
    "replay_service-delete_replay_session_v1",
    "replay_service-drive_replay_session_v1",
    "replay_service-get_replay_session_v1",
    "replay_service-list_replay_sessions_v1",
    "signal_service-get_signal_v1",
    "signal_service-list_signals_v1",
    "state_service-compute_state_v1",
    "stream_service-get_stream_v1",
    "stream_service-list_streams_v1",
];

/// The TCP sockets `pid` holds. `lsof` exits non-zero when there are none, so what
/// proves it ran is that it also finds the process's Unix sockets.
fn tcp_of(pid: u32) -> Vec<String> {
    let all = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-U"])
        .output();
    let all = String::from_utf8(all.unwrap().stdout).unwrap();
    assert!(
        all.contains("unix"),
        "lsof saw nothing of {pid}: a blind check must fail"
    );
    let tcp = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP"])
        .output();
    let tcp = String::from_utf8(tcp.unwrap().stdout).unwrap();
    tcp.lines().skip(1).map(str::to_owned).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listing_is_exactly_the_reads_and_no_write_can_be_called() {
    let norn = Norn::both("list").await;
    let c = norn.unix().await;
    let mut names: Vec<String> = c.tools().await.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(names, TOOLS);
    assert_eq!(norn_server::read_only_tool_names().len(), TOOLS.len());
    for tool in c.tools().await {
        assert_eq!(
            tool.annotations.unwrap().read_only_hint,
            Some(true),
            "{}",
            tool.name
        );
        assert!(tool.description.is_some_and(|d| !d.is_empty()));
    }
    for write in [
        "entity_service-create_entity_v1",
        "entity_service-batch_create_entities_v1",
        "signal_service-batch_create_signals_v1",
        "entity_service-delete_entity_v1",
        "stream_service-compact_stream_v1",
        "stream_service-apply_stream_retention_v1",
        "replay_service-step_replay_session_v1",
    ] {
        let args = json!({"parent": "x"}).as_object().unwrap().clone();
        let call =
            c.0.call_tool(CallToolRequestParams::new(write).with_arguments(args));
        assert!(call.await.is_err(), "{write} is not a tool");
    }
    norn.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_default_daemon_opens_no_tcp_port_and_no_mcp_socket() {
    let root = temp("default");
    let norn = Norn::start(root.clone(), &[]).await;
    assert_eq!(tcp_of(norn.child.id().unwrap()), Vec::<String>::new());
    assert!(!mcp_socket(&root).exists());
    norn.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_on_stdio_and_a_socket_opens_no_tcp_port_and_the_socket_is_owner_only() {
    let norn = Norn::both("notcp").await;
    let _client = norn.unix().await;
    assert_eq!(tcp_of(norn.child.id().unwrap()), Vec::<String>::new());
    let mode = std::fs::metadata(mcp_socket(&norn.root))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    norn.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stdio_client_leaving_stops_the_daemon_cleanly() {
    let mut norn = Norn::both("leave").await;
    let client = norn.stdio().await;
    client.0.cancel().await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = norn.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "norn outlived its client");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(status.success(), "{status:?}");
    assert!(!mcp_socket(&norn.root).exists(), "the socket is removed");
    norn.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_mcp_did_not_open_is_out_of_reach_and_its_own_are_bounded() {
    let norn = Norn::both("own").await;
    let interval = norn_protocol::google::r#type::Interval {
        start_time: Some(prost_types::Timestamp {
            seconds: 0,
            nanos: 0,
        }),
        end_time: Some(prost_types::Timestamp {
            seconds: 9,
            nanos: 0,
        }),
    };
    let theirs = ReplaySession {
        interval: Some(interval),
        scope: "entities/-".into(),
        pace: Some(Pace::Speed(0.0)),
        ..ReplaySession::default()
    };
    let request = CreateReplaySessionRequest {
        replay_session_id: "theirs".into(),
        replay_session: Some(theirs),
    };
    let mut grpc = ReplayServiceClient::new(norn.channel.clone());
    grpc.create_replay_session(request).await.unwrap();
    let c = norn.unix().await;
    let delete = "replay_service-delete_replay_session_v1";
    let args = json!({"name": "replaySessions/theirs"});
    assert_eq!(c.fails(delete, args).await["reason"], "MCP_UNKNOWN_SESSION");
    let session = json!({"interval": {"startTime": "1970-01-01T00:00:00Z",
        "endTime": "1970-01-01T00:00:09Z"}, "scope": "entities/-", "speed": 0.0});
    let create = "replay_service-create_replay_session_v1";
    for _ in 0..8 {
        c.ok(create, json!({"replay_session": session})).await;
    }
    let refused = c.fails(create, json!({"replay_session": session})).await;
    assert_eq!(refused["reason"], "MCP_LIMIT_EXCEEDED");
    norn.finish().await;
}

#[test]
fn a_non_loopback_address_is_refused_at_the_command_line() {
    let run = Command::new(env!("CARGO_BIN_EXE_norn"))
        .args(["--mcp", "http:0.0.0.0:8090"])
        .output()
        .unwrap();
    assert!(!run.status.success());
    let stderr = String::from_utf8(run.stderr).unwrap();
    assert!(stderr.contains("loopback"), "{stderr}");
}
