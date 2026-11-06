//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `backup` and `restore` of an encrypted database: the real binary, and a real
//! daemon. A backup copies segments as opaque bytes and holds no key, so what it
//! restores is still encrypted, and opens only under the key it was written with.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "offline_common/mod.rs"]
mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Scratch, append, config, count, declare, endpoint, ok, stop};
use norn_ipc::Endpoint;
use norn_runtime::Health;
use norn_server::Daemon;
use norn_server::config::KeySource;
use tokio_util::sync::CancellationToken;

const KEY: &str = "cc00000000000000000000000000000000000000000000000000000000000000";

/// An owner-only key file, beside the scratch directory rather than in it: the
/// daemon creates that directory itself, owner-only, for its socket.
struct KeyFile(PathBuf);

impl KeyFile {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nb-{name}-{}.key", std::process::id()));
        std::fs::write(&path, KEY).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Self(path)
    }
}

impl Drop for KeyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn serving_under(
    e: &Endpoint,
    data: &Path,
    key: &KeyFile,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let source = Some(KeySource::File(key.0.clone()));
    let daemon = Daemon::bind(config(e, data).key_source_set(source))
        .await
        .unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let task = tokio::spawn(async move {
        daemon.run(stop).await.unwrap();
    });
    health.reached(Health::Serving).await;
    (cancel, task)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_encrypted_backup_restores_and_opens_only_under_its_key() {
    let dir = Scratch::new("crypt");
    let key = KeyFile::new("crypt");
    let (e, data) = (endpoint(&dir.0), dir.0.join("data"));
    let daemon = serving_under(&e, &data, &key).await;
    declare(&e);
    append(&e, 0, 10);
    let b = dir.0.join("b");
    let said = ok(&e, &["backup", "--to", b.to_str().unwrap()]);
    assert!(said.contains("streams=1"), "{said}");
    stop(daemon).await;

    // Restore checks the backup's checksums and needs no key.
    let restored = dir.0.join("restored");
    ok(
        &e,
        &[
            "restore",
            "--from",
            b.to_str().unwrap(),
            "--data-dir",
            restored.to_str().unwrap(),
        ],
    );

    // Without the key, the restored segments refuse to open, and nothing is changed.
    let plain = Daemon::bind(config(&e, &restored))
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(plain.contains("is encrypted"), "{plain}");

    let daemon = serving_under(&e, &restored, &key).await;
    assert_eq!(count(&e), 10);
    let verify = ok(&e, &["verify", "plant"]);
    assert!(verify.contains("damaged_chunks=0"), "{verify}");
    stop(daemon).await;
    // The keyed start marked the restored database encrypted again.
    assert!(restored.join("ENCRYPTION").exists());
}
