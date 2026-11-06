//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A socket of its own per test, a running sink to it, and readings to push.

use crate::fake::{Log, Step};
use norn_ipc::Endpoint;
use norn_sink::{Catalog, Entity, Kind, Options, Reading, Report, Signal, Sink, ValueType};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

pub struct Socket(PathBuf);

impl Socket {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nk-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Norn binds only in a directory no one else may enter.
        let owner_only = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&dir, owner_only).unwrap();
        Self(dir)
    }

    pub fn endpoint(&self) -> Endpoint {
        format!("unix:{}", self.0.join("n.sock").display())
            .parse()
            .unwrap()
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A sink being run, on a task of the test's.
pub struct Running {
    pub sink: Sink,
    task: JoinHandle<Result<Report, norn_sink::SinkError>>,
}

impl Running {
    /// Closes the sink, and how its run ended.
    pub async fn close(self) -> Report {
        // Lets the run start, for a test that closes at once.
        tokio::task::yield_now().await;
        self.sink.close().await;
        self.task.await.unwrap().unwrap()
    }
}

/// Runs `sink`.
pub fn run(sink: Sink) -> Running {
    let task = tokio::spawn({
        let sink = sink.clone();
        async move { sink.run().await }
    });
    Running { sink, task }
}

/// A running sink with `options` that knows the signal [`readings`] are of.
pub fn started(endpoint: Endpoint, options: Options) -> Running {
    let sink = Sink::new(endpoint, "plant", options).unwrap();
    let catalog =
        Catalog::new()
            .entity(Entity::new("e"))
            .signal(Signal::new("e", "s", ValueType::Double));
    sink.declare(&catalog).unwrap();
    run(sink)
}

/// As [`started`], holding `bytes` at most.
pub fn holding(endpoint: Endpoint, bytes: usize) -> Running {
    started(endpoint, Options::default().buffer_bytes(bytes))
}

/// `n` readings of `entities/e/signals/s` from time `from`.
pub fn readings(from: i64, n: i64) -> Vec<Reading> {
    (from..from + n)
        .map(|i| {
            let at = prost_types::Timestamp {
                seconds: 1_000 + i,
                nanos: 0,
            };
            Reading::new("e", "s", at, Kind::DoubleValue(1.0))
        })
        .collect()
}

pub fn scripted(steps: &[Step]) -> Log {
    let log = Log::default();
    log.lock().unwrap().script.extend(steps.iter().copied());
    log
}

/// Waits until `sink` has `n` acknowledged, or fails with what it has.
pub async fn acknowledged(sink: &Sink, n: u64) {
    let start = Instant::now();
    while sink.counts().acknowledged < n {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{:?}, wanted {n}",
            sink.counts()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
