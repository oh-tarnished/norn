//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A daemon with a chosen engine configuration, a client appending through the
//! public API, and the real `nornctl` with a hard deadline: what the field runs
//! that found these bugs looked like, in a temporary directory.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use norn_ipc::Endpoint;
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest};
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Drains a child pipe while it runs, so a full pipe cannot block its exit.
fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).unwrap();
        bytes
    })
}

/// A running daemon on a Unix socket in a fresh directory.
pub struct Field {
    pub endpoint: Endpoint,
    pub dir: PathBuf,
    pub channel: Channel,
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl Field {
    /// A daemon configured by `toml`, a configuration file's text.
    pub async fn start(toml: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nfld{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let endpoint = Endpoint::Unix(dir.join("s").join("n.sock"));
        let config = DaemonConfig::from_toml(toml)
            .unwrap()
            .listen(endpoint.clone())
            .data_dir(dir.join("data"));
        let daemon = Daemon::bind(config).await.unwrap();
        let mut health = daemon.health();
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let task = tokio::spawn(async move {
            daemon.run(stop).await.unwrap();
        });
        assert_eq!(health.reached(Health::Serving).await, Health::Serving);
        let channel = norn_ipc::connect(&endpoint).await.unwrap();
        Self {
            endpoint,
            dir,
            channel,
            cancel,
            task: Some(task),
        }
    }

    /// `nornctl WORDS`, killed if it runs past `deadline`: a hang fails the
    /// test instead of hanging it.
    pub fn nornctl(&self, words: &[&str], deadline: Duration) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nornctl"))
            .args(["--endpoint", &self.endpoint.to_string(), "--timeout", "30"])
            .args(words)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = drain(child.stdout.take().unwrap());
        let stderr = drain(child.stderr.take().unwrap());
        let started = Instant::now();
        let mut timed_out = false;
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > deadline {
                let _ = child.kill();
                timed_out = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = Output {
            status: child.wait().unwrap(),
            stdout: stdout.join().unwrap(),
            stderr: stderr.join().unwrap(),
        };
        assert!(
            !timed_out,
            "nornctl {words:?} still running after {deadline:?}; it printed:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    /// `nornctl WORDS`, which must succeed within a minute; its stdout.
    pub fn ok(&self, words: &[&str]) -> String {
        let out = self.nornctl(words, Duration::from_secs(60));
        assert!(out.status.success(), "{words:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    /// `streams/plant`, `entities/ahu`, and a double signal for each of `ids`.
    pub fn seed(&self, ids: &[&str]) {
        self.ok(&["streams", "create", "plant"]);
        self.ok(&["entities", "create", "ahu"]);
        for id in ids {
            self.ok(&["signals", "create", "ahu", id, "plant", "double"]);
        }
    }

    /// Appends `samples` in one batch, waiting for `PERSISTED`.
    pub async fn append(&self, batch_id: i64, samples: Vec<Sample>) {
        let request = AppendSamplesRequest {
            stream: "streams/plant".into(),
            batch_id,
            samples,
            ack_level: AcknowledgementLevel::Persisted as i32,
        };
        let input = tonic::codegen::tokio_stream::iter(vec![request]);
        let mut client = TelemetryServiceClient::new(self.channel.clone());
        let mut replies = client.append_samples(input).await.unwrap().into_inner();
        while let Some(reply) = replies.message().await.unwrap() {
            assert!(reply.error.is_none(), "{reply:?}");
        }
    }

    /// Stops the daemon and removes its directory.
    pub async fn stop(mut self) {
        self.cancel.cancel();
        if let Some(task) = self.task.take() {
            task.await.unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The wall clock, as nanoseconds since the epoch.
pub fn now_nanos() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(since.as_nanos()).unwrap()
}

/// A good double sample of `entities/ahu/signals/{id}` at `nanos`.
pub fn sample(id: &str, nanos: i64, sequence: i64) -> Sample {
    Sample {
        signal: format!("entities/ahu/signals/{id}"),
        observe_time: Some(prost_types::Timestamp {
            seconds: nanos.div_euclid(1_000_000_000),
            nanos: i32::try_from(nanos.rem_euclid(1_000_000_000)).unwrap(),
        }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(sequence as f64)),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence,
    }
}
