//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! AdminService, health and the UNIMPLEMENTED stubs, over a real endpoint.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicUsize, Ordering};

use norn_ipc::Endpoint;
use norn_protocol::PROTOCOL_VERSION;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, GetServerRequest, NegotiateProtocolRequest};
use norn_protocol::rpc::StatusExt;
use norn_protocol::stream::ExportStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_runtime::{Health, RunReport};
use norn_server::{Daemon, DaemonConfig, DaemonError, ERROR_DOMAIN};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::Code;
use tonic::transport::Channel;

struct Running {
    channel: Channel,
    cancel: CancellationToken,
    task: JoinHandle<Result<RunReport, DaemonError>>,
    data: std::path::PathBuf,
}

/// Loopback TCP keeps this test identical on every OS; the Unix-socket
/// path is exercised end to end by nornctl's integration test.
async fn start() -> Running {
    let listen: Endpoint = "tcp:127.0.0.1:0".parse().unwrap();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let data = std::env::temp_dir().join(format!("norn-admin-{}-{n}", std::process::id()));
    let config = DaemonConfig::default()
        .listen(listen)
        .data_dir(data.clone());
    let daemon = Daemon::bind(config).await.unwrap();
    let endpoint = daemon.local_endpoint().clone();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    assert_eq!(health.reached(Health::Serving).await, Health::Serving);
    let channel = norn_ipc::connect(&endpoint).await.unwrap();
    Running {
        channel,
        cancel,
        task,
        data,
    }
}

impl Running {
    async fn stop(self) {
        self.cancel.cancel();
        let report = self.task.await.unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&self.data);
        assert_eq!(report.aborted, 0);
    }
}

fn negotiate(major: i32, required: Vec<i32>) -> NegotiateProtocolRequest {
    NegotiateProtocolRequest {
        name: "server".into(),
        client_version: Some(norn_protocol::admin::ProtocolVersion { major, minor: 7 }),
        required_capabilities: required,
        user_agent: "test".into(),
    }
}

#[tokio::test]
async fn get_server_describes_the_daemon_and_its_limits() {
    let d = start().await;
    let mut admin = AdminServiceClient::new(d.channel.clone());
    let server = admin
        .get_server(GetServerRequest {
            name: "server".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(server.name, "server");
    assert_eq!(server.server_version, norn_server::SERVER_VERSION);
    assert_eq!(server.protocol_version, Some(PROTOCOL_VERSION.into()));
    let limits = server.limits.unwrap();
    assert_eq!(limits.max_streams, 1024);
    assert!(server.start_time.is_some());
    let bad = admin
        .get_server(GetServerRequest {
            name: "servers".into(),
        })
        .await;
    assert_eq!(bad.unwrap_err().code(), Code::InvalidArgument);
    d.stop().await;
}

#[tokio::test]
async fn negotiation_agrees_the_lower_minor_of_the_same_major() {
    let d = start().await;
    let mut admin = AdminServiceClient::new(d.channel.clone());
    let agreed = admin
        .negotiate_protocol(negotiate(PROTOCOL_VERSION.major, vec![]))
        .await
        .unwrap()
        .into_inner();
    let v = agreed.protocol_version.unwrap();
    assert_eq!(
        (v.major, v.minor),
        (PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor.min(7))
    );
    d.stop().await;
}

#[tokio::test]
async fn another_major_fails_with_the_version_reason_and_the_servers_version() {
    let d = start().await;
    let mut admin = AdminServiceClient::new(d.channel.clone());
    let err = admin
        .negotiate_protocol(negotiate(99, vec![]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    let info = err.get_details_error_info().unwrap();
    assert_eq!(info.domain, ERROR_DOMAIN);
    assert_eq!(info.reason, "ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED");
    assert_eq!(
        info.metadata["server_major"],
        PROTOCOL_VERSION.major.to_string()
    );
    d.stop().await;
}

#[tokio::test]
async fn a_required_capability_not_offered_fails_and_names_it() {
    let d = start().await;
    let mut admin = AdminServiceClient::new(d.channel.clone());
    let required = vec![Capability::SharedMemory as i32];
    let err = admin
        .negotiate_protocol(negotiate(PROTOCOL_VERSION.major, required))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    let info = err.get_details_error_info().unwrap();
    assert_eq!(info.reason, "ERROR_REASON_CAPABILITY_UNAVAILABLE");
    assert_eq!(info.metadata["capability"], "CAPABILITY_SHARED_MEMORY");
    d.stop().await;
}

#[tokio::test]
async fn an_unbuilt_service_is_unimplemented_with_error_info() {
    let d = start().await;
    let mut streams = StreamServiceClient::new(d.channel.clone());
    let err = streams
        .export_stream(ExportStreamRequest {
            name: "streams/a".into(),
            ..ExportStreamRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unimplemented);
    let info = err.get_details_error_info().unwrap();
    assert_eq!(info.domain, ERROR_DOMAIN);
    assert_eq!(info.reason, "ERROR_REASON_NOT_IMPLEMENTED");
    assert_eq!(info.metadata["service"], "norn.stream.v1.StreamService");
    assert_eq!(info.metadata["method"], "ExportStream");
    assert_eq!(info.metadata["capability"], "CAPABILITY_EXPORT");
    d.stop().await;
}
