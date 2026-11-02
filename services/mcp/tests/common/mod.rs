//! A real daemon, the real `norn-mcp` binary, and a real MCP client.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod client;
pub mod seed;

#[allow(unused_imports, reason = "each test binary uses some of them")]
pub use client::{Client, http, http_server, stdio};

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_ipc::Endpoint;
use norn_runtime::{Health, RunReport};
use norn_server::{Daemon, DaemonConfig, DaemonError};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// A fresh short directory: a Unix socket's path has a small limit.
pub fn temp(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nmcp-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The endpoint under `root`; the daemon creates its private directory.
#[cfg(unix)]
pub fn socket(root: &std::path::Path) -> Endpoint {
    Endpoint::Unix(root.join("ipc/n.sock"))
}

/// On Windows, where tokio has no Unix sockets, a named pipe unique to the
/// process and the call: the same path through `norn_ipc` the release takes.
#[cfg(windows)]
pub fn socket(_root: &std::path::Path) -> Endpoint {
    static PIPE: AtomicUsize = AtomicUsize::new(0);
    let n = PIPE.fetch_add(1, Ordering::Relaxed);
    let name = format!(r"pipe:\\.\pipe\norn-mcp-{}-{n}", std::process::id());
    name.parse().unwrap()
}

/// A daemon running in this process.
pub struct Running {
    pub endpoint: Endpoint,
    pub root: PathBuf,
    cancel: CancellationToken,
    task: JoinHandle<Result<RunReport, DaemonError>>,
}

impl Running {
    pub async fn start(root: PathBuf) -> Self {
        let endpoint = socket(&root);
        let config = DaemonConfig::default()
            .listen(endpoint.clone())
            .data_dir(root.join("data"));
        let daemon = Daemon::bind(config).await.unwrap();
        let mut health = daemon.health();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(daemon.run(cancel.clone()));
        health.reached(Health::Serving).await;
        Self {
            endpoint,
            root,
            cancel,
            task,
        }
    }

    /// Stops the daemon, keeping its files.
    pub async fn stop(self) -> PathBuf {
        self.cancel.cancel();
        self.task.await.unwrap().unwrap();
        self.root
    }

    /// Stops the daemon and removes everything.
    pub async fn finish(self) {
        let root = self.stop().await;
        let _ = std::fs::remove_dir_all(root);
    }

    pub async fn channel(&self) -> tonic::transport::Channel {
        norn_ipc::connect(&self.endpoint).await.unwrap()
    }
}
