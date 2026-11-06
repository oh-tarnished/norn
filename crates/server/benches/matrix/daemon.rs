//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A real `norn` on a fresh data directory under the run's data root, with its
//! own socket: never the user's daemon, never its socket. Removed on drop.
//!
//! The socket lives in a short directory of its own under `/tmp`, not under the
//! data root: a Unix socket path is limited to about 104 bytes, and the data root
//! is whatever disk the owner wants to measure.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tonic::transport::Channel;

use crate::driver::connect;

/// The one configuration every daemon of the matrix runs, recorded in the
/// report. Defaults everywhere else: the budgets are what is being measured.
/// Background upkeep is off so compaction runs only when it is timed; the
/// signal limit fits the 100,000-signal row; rings are offered to whoever asks.
pub const CONFIG: &str = "[limits]\nmax_signals = 200000\n\n\
                          [maintenance]\nbackground = false\n\n\
                          [shared_memory]\nenabled = true\n";

/// The longest a start may take before the run gives up: a recovery replaying a
/// long journal on the full profile is the slowest.
const START_LIMIT: Duration = Duration::from_secs(900);

/// How often a start checks that the daemon is still alive.
const LIVENESS: Duration = Duration::from_millis(200);

static DAEMONS: AtomicU32 = AtomicU32::new(0);

pub struct Daemon {
    child: Option<Child>,
    pub root: PathBuf,
    sockets: PathBuf,
    pub channel: Channel,
}

fn spawn(root: &Path, sockets: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--config")
        .arg(root.join("norn.toml"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .arg("--listen")
        .arg(format!("unix:{}", sockets.join("n.sock").display()))
        .args(["--log-level", "error"])
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

/// Waits for the first answer. A daemon that exits, or never answers, stops the
/// run with the reason instead of leaving it waiting.
async fn answering(sockets: &Path, child: &mut Child) -> Channel {
    let began = Instant::now();
    let socket = sockets.join("n.sock");
    let connected = connect(&socket);
    tokio::pin!(connected);
    loop {
        tokio::select! {
            channel = &mut connected => return channel,
            () = tokio::time::sleep(LIVENESS) => {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("norn exited during its start: {status}");
                }
                if began.elapsed() > START_LIMIT {
                    let _ = child.kill();
                    panic!("norn did not answer within {START_LIMIT:?}");
                }
            }
        }
    }
}

impl Daemon {
    /// Starts a daemon in a new directory `base/<tag>`; returns it and the time
    /// from spawning to its first `GetServer` answer.
    pub async fn start(base: &Path, tag: &str) -> (Self, Duration) {
        let root = base.join(format!("norn-matrix-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("norn.toml"), CONFIG).unwrap();
        let serial = DAEMONS.fetch_add(1, Ordering::Relaxed);
        let sockets = PathBuf::from(format!("/tmp/nm-{}-{serial}", std::process::id()));
        let began = Instant::now();
        let mut child = spawn(&root, &sockets);
        let channel = answering(&sockets, &mut child).await;
        let took = began.elapsed();
        let daemon = Self {
            child: Some(child),
            root,
            sockets,
            channel,
        };
        (daemon, took)
    }

    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    /// The stream files: what bytes per sample is measured over.
    pub fn streams(&self) -> PathBuf {
        self.root.join("data/streams")
    }

    /// A clean stop, SIGTERM: the open segment seals and the marker is written.
    pub fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            let id = child.id().to_string();
            Command::new("kill").args(["-TERM", &id]).status().unwrap();
            assert!(
                child.wait().unwrap().success(),
                "norn failed to stop cleanly"
            );
        }
    }

    /// `kill -9`: no seal, no marker, whatever was in flight.
    pub fn kill9(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill().unwrap();
            let _ = child.wait();
        }
    }

    /// Starts again on the same directory; returns the time to the first answer.
    pub async fn restart(&mut self) -> Duration {
        assert!(self.child.is_none(), "stop the daemon before restarting it");
        let began = Instant::now();
        let mut child = spawn(&self.root, &self.sockets);
        self.channel = answering(&self.sockets, &mut child).await;
        let took = began.elapsed();
        self.child = Some(child);
        took
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.kill9();
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.sockets);
    }
}
