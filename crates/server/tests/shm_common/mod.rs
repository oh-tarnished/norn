//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A daemon on a Unix socket with `[shared_memory]` as a test asks, and a
//! client's view of the plane.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use norn_ipc::{AppendCall, AppendOptions, Endpoint};
use norn_protocol::PROTOCOL_VERSION;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, NegotiateProtocolRequest, NegotiateProtocolResponse};
use norn_protocol::common::ValueType;
use norn_protocol::telemetry::AppendSamplesResponse;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use norn_shared_memory::SharedMemory;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

use super::common::{Running, seed, temp};

/// A daemon configured by `toml` (a whole file), on a Unix socket in its own
/// directory, seeded with `entities/dev/signals/t` in `streams/plant`.
pub async fn start(toml: &str) -> Running {
    let dir = temp("shm");
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("ipc/n.sock");
    let config = DaemonConfig::from_toml(toml)
        .unwrap()
        .listen(Endpoint::Unix(socket.clone()));
    let running = start_with(config, dir.join("data"), &socket).await;
    seed(&running.channel, &[("t", ValueType::Double)]).await;
    running
}

/// `config` served from `data` on `socket`.
pub async fn start_with(config: DaemonConfig, data: PathBuf, socket: &Path) -> Running {
    let daemon = Daemon::bind(config.data_dir(data.clone())).await.unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    health.reached(Health::Serving).await;
    let channel = norn_ipc::connect(&Endpoint::Unix(socket.to_path_buf()))
        .await
        .unwrap();
    Running {
        channel,
        cancel,
        task,
        data,
    }
}

/// The `[shared_memory]` table turned on, with `extra` lines.
pub fn on(extra: &str) -> String {
    format!("[shared_memory]\nenabled = true\n{extra}\n")
}

/// The client's opt-in.
pub fn options() -> AppendOptions {
    AppendOptions::default()
        .capacity(4)
        .shared_memory(Arc::new(SharedMemory))
}

/// A call that asks for a ring.
pub async fn call(channel: &Channel) -> AppendCall {
    AppendCall::open(channel.clone(), &options()).await.unwrap()
}

/// What `NegotiateProtocol` says at this crate's version.
pub async fn negotiated(channel: &Channel) -> NegotiateProtocolResponse {
    let request = NegotiateProtocolRequest {
        name: "server".into(),
        client_version: Some(PROTOCOL_VERSION.into()),
        required_capabilities: vec![],
        user_agent: "shm-test".into(),
    };
    AdminServiceClient::new(channel.clone())
        .negotiate_protocol(request)
        .await
        .unwrap()
        .into_inner()
}

/// Whether the daemon offers the plane, by capability and by description.
pub async fn offered(channel: &Channel) -> bool {
    let reply = negotiated(channel).await;
    let capability = reply
        .capabilities
        .contains(&(Capability::SharedMemory as i32));
    assert_eq!(capability, reply.shared_memory.is_some(), "{reply:?}");
    capability
}

/// Closes the call's request side and collects every response.
pub async fn finish(call: AppendCall) -> Vec<AppendSamplesResponse> {
    let (sender, mut replies) = call.into_parts();
    drop(sender);
    let mut out = Vec::new();
    while let Some(reply) = replies.message().await.unwrap() {
        out.push(reply);
    }
    out
}

/// Waits until `check` holds, or fails the test.
pub async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check().await {
        assert!(Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
