//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A real daemon on an encrypted store: restart, crash, wrong key, verify, rekey.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod query_common;
mod upkeep_common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{append, batch, double, seed, start, temp};
use norn_protocol::common::ValueType;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_server::config::KeySource;
use norn_server::{Daemon, DaemonConfig};
use query_common::{all, samples};
use upkeep_common::{segments, server, verify};

const A: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
const B: &str = "bb00000000000000000000000000000000000000000000000000000000000000";

/// An owner-only key file holding `text`, next to `data`.
fn key_file(data: &Path, tag: &str, text: &str) -> PathBuf {
    let path = data.with_extension(format!("{tag}.key"));
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path
}

fn under(key: &Path) -> DaemonConfig {
    DaemonConfig::default().key_source_set(Some(KeySource::File(key.to_path_buf())))
}

/// A store under `key` holding ten samples, in one sealed segment.
async fn seeded(tag: &str, key_text: &str) -> (PathBuf, PathBuf) {
    let data = temp(tag);
    let key = key_file(&data, "first", key_text);
    let d = start(under(&key), data).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let list = (0..10).map(|n| double("t", n)).collect();
    append(&d.channel, vec![batch(1, list, Durable)]).await;
    (d.stop_keeping().await, key)
}

fn refused(config: DaemonConfig, data: &Path) -> impl Future<Output = String> + use<> {
    let config = config
        .data_dir(data.to_path_buf())
        .listen("tcp:127.0.0.1:0".parse().unwrap());
    async move {
        Daemon::bind(config)
            .await
            .expect_err("must be refused")
            .to_string()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn samples_survive_a_restart_and_what_is_stored_is_encrypted() {
    let (data, key) = seeded("enc-restart", A).await;
    let raw = segments(&data);
    assert!(!raw.is_empty());
    let bytes = std::fs::read(&raw[0]).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 257);
    let d = start(under(&key), data).await;
    assert_eq!(samples(&d.channel, all()).await.len(), 10);
    assert_eq!(verify(&d.channel).await.damaged_chunk_count, 0);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_key_or_no_key_stops_the_start_and_changes_nothing() {
    let (data, _key) = seeded("enc-wrong", A).await;
    let raw = segments(&data);
    let before = std::fs::read(&raw[0]).unwrap();
    let wrong = key_file(&data, "wrong", B);
    assert!(refused(under(&wrong), &data).await.contains("wrong key"));
    let none = refused(DaemonConfig::default(), &data).await;
    assert!(none.contains("is encrypted"), "{none}");
    assert_eq!(before, std::fs::read(&raw[0]).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_store_opened_with_encryption_on_is_refused() {
    let d = start(DaemonConfig::default(), temp("enc-plain")).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let list = (0..10).map(|n| double("t", n)).collect();
    append(&d.channel, vec![batch(1, list, Durable)]).await;
    let data = d.stop_keeping().await;
    let key = key_file(&data, "k", A);
    assert!(refused(under(&key), &data).await.contains("not encrypted"));
    let d = start(DaemonConfig::default(), data).await;
    assert_eq!(
        samples(&d.channel, all()).await.len(),
        10,
        "still opens plain"
    );
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kill_loses_nothing_durable_and_journals_replay_under_the_key() {
    let (data, key) = seeded("enc-crash", A).await;
    let root = temp("enc-crash-ipc");
    std::fs::create_dir_all(&root).unwrap();
    let socket = root.join("ipc/n.sock");
    let config = root.join("norn.toml");
    let text = format!(
        "[storage]\nencryption = \"on\"\nkey_file = \"{}\"\n",
        key.display()
    );
    std::fs::write(&config, text).unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--config")
        .arg(&config)
        .arg("--data-dir")
        .arg(&data)
        .arg("--listen")
        .arg(format!("unix:{}", socket.display()))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let channel = connect(&socket).await;
    let list = (10..20).map(|n| double("t", n)).collect();
    append(&channel, vec![batch(1, list, Durable)]).await;
    drop(channel);
    daemon.kill().unwrap();
    daemon.wait().unwrap();

    let d = start(under(&key), data).await;
    assert_eq!(
        server(&d.channel)
            .await
            .recovery
            .unwrap()
            .replayed_sample_count,
        10
    );
    assert_eq!(samples(&d.channel, all()).await.len(), 20);
    assert_eq!(verify(&d.channel).await.damaged_chunk_count, 0);
    d.stop().await;
    let _ = std::fs::remove_dir_all(root);
}

async fn connect(socket: &Path) -> tonic::transport::Channel {
    let endpoint = norn_ipc::Endpoint::Unix(socket.to_path_buf());
    for _ in 0..600 {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            return channel;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the daemon never answered")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_file_others_can_read_stops_the_start() {
    let data = temp("enc-mode");
    let key = key_file(&data, "open", A);
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(refused(under(&key), &data).await.contains("chmod 600"));
}
