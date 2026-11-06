//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An encrypted store's upkeep: verify finds forged chunks, `norn rekey` rotates.
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
use std::process::Command;

use common::{append, batch, double, seed, start, temp};
use norn_protocol::common::ValueType;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_server::DaemonConfig;
use norn_server::config::KeySource;
use norn_storage::{SegmentReader, StorageOptions};
use query_common::{all, samples};
use upkeep_common::{segments, verify};

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

#[tokio::test(flavor = "multi_thread")]
async fn norn_rekey_moves_the_store_to_the_new_key_and_the_old_key_can_go() {
    let (data, _old) = seeded("enc-rekey", A).await;
    let both = key_file(&data, "both", &format!("{B}\n{A}\n"));
    let config = data.with_extension("toml");
    let text = format!(
        "[storage]\nencryption = \"on\"\nkey_file = \"{}\"\n",
        both.display()
    );
    std::fs::write(&config, text).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("rekey")
        .arg("--config")
        .arg(&config)
        .arg("--data-dir")
        .arg(&data)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("left=0"), "{out}");
    assert!(!out.contains("rewritten=0"), "{out}");
    let new_only = key_file(&data, "new", B);
    let d = start(under(&new_only), data).await;
    assert_eq!(samples(&d.channel, all()).await.len(), 10);
    assert_eq!(verify(&d.channel).await.damaged_chunk_count, 0);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_reports_a_damaged_encrypted_chunk() {
    let (data, key) = seeded("enc-damage", A).await;
    let raw = segments(&data);
    let mut keyed = StorageOptions::default();
    keyed.keyring = Some(norn_storage::Keyring::parse(A).unwrap().leak());
    let meta = SegmentReader::open(&raw[0], keyed).unwrap().chunks()[0];
    let mut bytes = std::fs::read(&raw[0]).unwrap();
    let at = meta.offset as usize + 60;
    bytes[at] ^= 1;
    let start_at = meta.offset as usize;
    let end = start_at + meta.frame_len() as usize;
    let crc = norn_storage::crc32c(&bytes[start_at + 48..end]);
    bytes[start_at + 40..start_at + 44].copy_from_slice(&crc.to_le_bytes());
    let head = norn_storage::crc32c(&bytes[start_at..start_at + 44]);
    bytes[start_at + 44..start_at + 48].copy_from_slice(&head.to_le_bytes());
    std::fs::write(&raw[0], bytes).unwrap();
    let d = start(under(&key), data).await;
    let found = verify(&d.channel).await;
    assert_eq!(found.damaged_chunk_count, 1);
    assert!(format!("{found:?}").contains("Authentication"), "{found:?}");
    d.stop().await;
}
