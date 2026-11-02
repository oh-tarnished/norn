//! The real `norn` binary as a child process, for tests that kill it.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use tokio::sync::mpsc;
use tonic::transport::Channel;

use super::common::{Requests, batch, double};

pub fn spawn(data: &Path, socket: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(data)
        .arg("--listen")
        .arg(format!("unix:{}", socket.display()))
        .args(["--log-level", "error"])
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

pub async fn connect(socket: &Path) -> Channel {
    let endpoint = Endpoint::Unix(socket.to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            let get = GetServerRequest {
                name: "server".into(),
            };
            let mut admin = AdminServiceClient::new(channel.clone());
            if admin.get_server(get).await.is_ok() {
                return channel;
            }
        }
        assert!(Instant::now() < deadline, "the daemon never answered");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Sends batches of `per` samples of `signal`, sequence n at time 1000 + n, until
/// the call breaks; returns the ids of batches acknowledged `DURABLE`.
pub async fn ingest_until_killed(
    channel: Channel,
    signal: &'static str,
    per: i64,
) -> BTreeSet<i64> {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        for id in 1.. {
            let from = (id - 1) * per;
            let list = (from..from + per).map(|n| double(signal, n)).collect();
            if tx.send(batch(id, list, Durable)).await.is_err() {
                return;
            }
        }
    });
    let mut client = TelemetryServiceClient::new(channel);
    let mut acked = BTreeSet::new();
    let Ok(replies) = client.append_samples(Requests(rx)).await else {
        return acked;
    };
    let mut replies = replies.into_inner();
    while let Ok(Some(r)) = replies.message().await {
        if r.ack_level == Durable as i32 && r.error.is_none() {
            acked.insert(r.batch_id);
        }
    }
    acked
}
