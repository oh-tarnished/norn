//! A real daemon on a temporary endpoint and data directory, removed afterwards.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_ipc::Endpoint;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory, removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("nx{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A running daemon.
pub struct Running {
    pub endpoint: Endpoint,
    pub dir: TempDir,
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl Running {
    #[cfg(unix)]
    fn wanted(dir: &TempDir) -> Endpoint {
        Endpoint::Unix(dir.0.join("s").join("n.sock"))
    }

    #[cfg(windows)]
    fn wanted(_: &TempDir) -> Endpoint {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!(r"pipe:\\.\pipe\norn-x-{}-{n}", std::process::id());
        name.parse().unwrap()
    }

    pub async fn start() -> Self {
        let dir = TempDir::new();
        let endpoint = Self::wanted(&dir);
        let config = DaemonConfig::default()
            .listen(endpoint.clone())
            .data_dir(dir.0.join("data"));
        let daemon = Daemon::bind(config).await.unwrap();
        let mut health = daemon.health();
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let task = tokio::spawn(async move {
            let _ = daemon.run(stop).await;
        });
        assert_eq!(health.reached(Health::Serving).await, Health::Serving);
        Self {
            endpoint,
            dir,
            cancel,
            task: Some(task),
        }
    }

    pub async fn channel(&self) -> tonic::transport::Channel {
        norn_ipc::connect(&self.endpoint).await.unwrap()
    }

    pub async fn stop(mut self) {
        self.cancel.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    /// Runs the real `nornctl`.
    pub fn nornctl(&self, words: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nornctl"))
            .args(["--endpoint", &self.endpoint.to_string(), "--timeout", "30"])
            .args(words)
            .output()
            .unwrap()
    }

    /// `nornctl`, which must succeed; its stdout.
    pub fn ok(&self, words: &[&str]) -> String {
        let output = self.nornctl(words);
        assert!(output.status.success(), "{words:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}
