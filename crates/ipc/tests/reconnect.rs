//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A client channel outlives its server: after the server stops and a new one
//! binds the same local endpoint, the next call on the same channel is answered.
//!
//! On Windows this is a named pipe, the case CI once saw fail for norn-mcp's
//! channel; on Unix the same body runs over a Unix socket. Both `connect` (eager)
//! and `connect_lazy` channels are checked, since tonic treats them differently
//! before their first connection.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{Running, check, start};
use norn_ipc::{Endpoint, ServerConfig, connect, connect_lazy};
use tonic_health::pb::health_check_response::ServingStatus;

const SERVING: i32 = ServingStatus::Serving as i32;

async fn stop(server: Running) {
    server.shutdown.cancel();
    server.task.await.unwrap().unwrap();
}

async fn serve(endpoint: &Endpoint) -> Running {
    let config = ServerConfig::default().drain_timeout(Duration::from_millis(500));
    start(endpoint, config, Duration::ZERO).await
}

/// The body: a call, the server gone, a call refused, the server back, a call
/// answered on the very same channels.
async fn a_channel_reconnects_after_its_server_restarts(endpoint: Endpoint) {
    let server = serve(&endpoint).await;
    let eager = connect(&endpoint).await.unwrap();
    let lazy = connect_lazy(&endpoint);
    assert_eq!(check(eager.clone()).await.unwrap(), SERVING);
    assert_eq!(check(lazy.clone()).await.unwrap(), SERVING);

    stop(server).await;
    assert!(check(eager.clone()).await.is_err(), "no server, no answer");
    assert!(check(lazy.clone()).await.is_err(), "no server, no answer");

    let server = serve(&endpoint).await;
    assert_eq!(check(eager).await.unwrap(), SERVING, "eager channel");
    assert_eq!(check(lazy).await.unwrap(), SERVING, "lazy channel");
    stop(server).await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn a_pipe_channel_reconnects_after_its_server_restarts() {
    let name = format!(r"\\.\pipe\norn-reconnect-{}", std::process::id());
    a_channel_reconnects_after_its_server_restarts(Endpoint::Pipe(name)).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_unix_socket_channel_reconnects_after_its_server_restarts() {
    let endpoint = common::unix_endpoint();
    let dir = common::unix_dir(&endpoint);
    a_channel_reconnects_after_its_server_restarts(endpoint).await;
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}
