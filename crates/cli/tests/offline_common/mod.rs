//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The real binary and a real daemon, for the backup and offline commands.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use norn_ipc::Endpoint;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio_util::sync::CancellationToken;

pub fn run(endpoint: &Endpoint, words: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args(["--endpoint", &endpoint.to_string(), "--timeout", "5"])
        .args(words)
        .output()
        .unwrap()
}

pub fn ok(endpoint: &Endpoint, words: &[&str]) -> String {
    let out = run(endpoint, words);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{words:?}: {text} {error}");
    text
}

pub fn refused(endpoint: &Endpoint, words: &[&str]) -> String {
    let out = run(endpoint, words);
    assert!(!out.status.success(), "{words:?} should have failed");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub fn config(endpoint: &Endpoint, data: &Path) -> DaemonConfig {
    DaemonConfig::default()
        .listen(endpoint.clone())
        .data_dir(data.to_path_buf())
}

pub async fn serving(
    endpoint: &Endpoint,
    data: &Path,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let daemon = Daemon::bind(config(endpoint, data)).await.unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let task = tokio::spawn(async move {
        daemon.run(stop).await.unwrap();
    });
    health.reached(Health::Serving).await;
    (cancel, task)
}

pub async fn stop(serving: (CancellationToken, tokio::task::JoinHandle<()>)) {
    serving.0.cancel();
    serving.1.await.unwrap();
}

pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        // A short path: it holds a Unix socket.
        let path = std::env::temp_dir().join(format!("nb-{name}-{}", std::process::id()));
        // Not created here: the daemon makes a socket's directory, owner-only.
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
pub fn endpoint(dir: &Path) -> Endpoint {
    Endpoint::Unix(dir.join("n.sock"))
}

#[cfg(windows)]
pub fn endpoint(dir: &Path) -> Endpoint {
    let leaf = dir.file_name().unwrap().to_string_lossy();
    let name = format!(r"pipe:\\.\pipe\norn-{leaf}");
    name.parse().unwrap()
}

pub fn declare(e: &Endpoint) {
    ok(e, &["streams", "create", "plant"]);
    ok(e, &["entities", "create", "line"]);
    ok(e, &["signals", "create", "line", "t", "plant", "double"]);
}

pub fn append(e: &Endpoint, from: u32, to: u32) {
    for n in from..to {
        let (value, at) = (n.to_string(), format!("{}", 1_700_000_000 + n));
        ok(
            e,
            &[
                "append",
                "plant",
                "entities/line/signals/t",
                &value,
                "--at",
                &at,
            ],
        );
    }
}

pub fn count(e: &Endpoint) -> u64 {
    let text = ok(
        e,
        &[
            "query",
            "entities/line/signals/t",
            "--agg",
            "count",
            "--from",
            "1700000000",
            "--to",
            "1800000000",
        ],
    );
    let found = text
        .split(|c: char| !c.is_ascii_digit())
        .rfind(|w| !w.is_empty());
    found
        .and_then(|w| w.parse().ok())
        .unwrap_or_else(|| panic!("no count in {text}"))
}
