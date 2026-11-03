//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! kill -9: a real daemon killed mid-ingest, several times over.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{Requests, batch, double, seed, stored, temp};
use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::common::ValueType;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use tokio::sync::mpsc;
use tonic::transport::Channel;

const ROUNDS: u32 = 5;
const SIGNALS: [&str; 4] = ["a", "b", "c", "d"];
const BATCH: i64 = 200;

fn spawn(data: &Path, socket: &Path) -> Child {
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

async fn connect(socket: &Path) -> Channel {
    let endpoint = Endpoint::Unix(socket.to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            let mut admin = AdminServiceClient::new(channel.clone());
            let get = GetServerRequest {
                name: "server".into(),
            };
            if admin.get_server(get).await.is_ok() {
                return channel;
            }
        }
        assert!(Instant::now() < deadline, "the daemon never answered");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Sends batches from sequence `from`, returning those acknowledged `DURABLE`
/// before the call broke.
async fn ingest(channel: Channel, from: i64) -> Vec<i64> {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        let mut start = from;
        let mut id = 1;
        loop {
            let samples = (start..start + BATCH)
                .map(|n| double(SIGNALS[(n % 4) as usize], n))
                .collect();
            if tx.send(batch(id, samples, Durable)).await.is_err() {
                return;
            }
            id += 1;
            start += BATCH;
        }
    });
    let mut client = TelemetryServiceClient::new(channel);
    let mut acked = Vec::new();
    let Ok(replies) = client.append_samples(Requests(rx)).await else {
        return acked;
    };
    let mut replies = replies.into_inner();
    while let Ok(Some(r)) = replies.message().await {
        if r.ack_level == Durable as i32 && r.error.is_none() {
            acked.push(from + (r.batch_id - 1) * BATCH);
        }
    }
    acked
}

#[tokio::test(flavor = "multi_thread")]
async fn every_batch_acknowledged_durable_survives_kill_9_sample_for_sample() {
    let root = temp("crash");
    let (data, socket) = (root.join("data"), root.join("ipc/n.sock"));
    let mut acked: Vec<i64> = Vec::new();
    let mut next = 0;
    for round in 0..ROUNDS {
        let mut daemon = spawn(&data, &socket);
        let channel = connect(&socket).await;
        if round == 0 {
            let kinds = SIGNALS.map(|s| (s, ValueType::Double));
            seed(&channel, &kinds).await;
        }
        let run = tokio::spawn(ingest(channel, next));
        tokio::time::sleep(Duration::from_millis(300 + u64::from(round) * 170)).await;
        daemon.kill().unwrap();
        daemon.wait().unwrap();
        let got = run.await.unwrap();
        assert!(!got.is_empty(), "round {round} acknowledged nothing");
        let last = got.iter().max().copied().unwrap();
        // What was in flight may or may not have landed; start clear of it.
        next = last + 1_000 * BATCH;
        acked.extend(got);
    }
    let mut daemon = spawn(&data, &socket);
    connect(&socket).await;
    Command::new("kill")
        .arg("-TERM")
        .arg(daemon.id().to_string())
        .status()
        .unwrap();
    assert!(daemon.wait().unwrap().success());

    let on_disk = stored(&data);
    let mut count: HashMap<u64, u32> = HashMap::new();
    let mut by_seq: HashMap<u64, f64> = HashMap::new();
    for s in &on_disk {
        *count.entry(s.sequence.get()).or_default() += 1;
        if let norn_core::Value::F64(v) = s.value {
            by_seq.insert(s.sequence.get(), v);
        }
    }
    let wanted: HashSet<i64> = acked
        .iter()
        .flat_map(|start| *start..*start + BATCH)
        .collect();
    assert!(wanted.len() >= 400, "too little ran: {}", wanted.len());
    for seq in &wanted {
        let key = *seq as u64;
        assert_eq!(count.get(&key), Some(&1), "sequence {seq}");
        assert_eq!(by_seq[&key], *seq as f64, "sequence {seq}'s value");
    }
    assert!(count.values().all(|c| *c == 1), "a sample was stored twice");
    println!(
        "acknowledged {} samples over {ROUNDS} kills; {} on disk",
        wanted.len(),
        on_disk.len()
    );
    let _ = std::fs::remove_dir_all(&root);
}
