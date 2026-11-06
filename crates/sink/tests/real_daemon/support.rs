//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A real daemon in a temporary directory, and what the tests declare and push.

use norn_ipc::Endpoint;
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use norn_sink::{Catalog, Entity, Kind, Reading, Signal, Sink, ValueType};
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nkr-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    pub fn endpoint(&self) -> Endpoint {
        Endpoint::Unix(self.0.join("ipc").join("n.sock"))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Running(CancellationToken, tokio::task::JoinHandle<()>);

pub async fn start(dir: &Dir, toml: &str) -> Running {
    let config = DaemonConfig::from_toml(toml)
        .unwrap()
        .listen(dir.endpoint())
        .data_dir(dir.0.join("data"));
    let daemon = Daemon::bind(config).await.unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let task = tokio::spawn(async move {
        let _ = daemon.run(stop).await;
    });
    assert_eq!(health.reached(Health::Serving).await, Health::Serving);
    Running(cancel, task)
}

impl Running {
    /// Stops it as `SIGTERM` does: the drain, and every acknowledged sample kept.
    pub async fn stop(self) {
        self.0.cancel();
        let _ = self.1.await;
    }
}

pub fn catalog(unit: &str, n: usize) -> Catalog {
    let mut catalog = Catalog::new().entity(Entity::new("rig").display_name("Rig"));
    for i in 0..n {
        catalog.push_signal(
            Signal::new("rig", format!("s{i}"), ValueType::Double)
                .unit(unit)
                .display_name(
                    format!("A signal with a name of some length, number {i}: ").repeat(4),
                ),
        );
    }
    catalog
}

pub fn sweep(round: i64, n: usize) -> Vec<Reading> {
    (0..n)
        .map(|i| {
            let at = prost_types::Timestamp {
                seconds: 1_700_000_000 + round,
                nanos: 0,
            };
            let id = format!("s{i}");
            Reading::new("rig", &id, at, Kind::DoubleValue(0.5)).sequence(round)
        })
        .collect()
}

pub async fn sample_count(endpoint: &Endpoint) -> i64 {
    let channel = norn_ipc::connect(endpoint).await.unwrap();
    let name = "streams/plant".to_owned();
    let stream = StreamServiceClient::new(channel)
        .get_stream(GetStreamRequest { name })
        .await
        .unwrap()
        .into_inner();
    stream.stats.unwrap().sample_count
}

pub async fn within<F: Future>(what: &str, f: F) -> F::Output {
    match tokio::time::timeout(Duration::from_secs(60), f).await {
        Ok(out) => out,
        Err(_) => panic!("{what} took over a minute"),
    }
}

pub fn spawned(
    sink: &Sink,
) -> tokio::task::JoinHandle<Result<norn_sink::Report, norn_sink::SinkError>> {
    let sink = sink.clone();
    tokio::spawn(async move { sink.run().await })
}
