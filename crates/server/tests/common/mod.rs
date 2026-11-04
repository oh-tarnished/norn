//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A real daemon in-process on loopback, and a client's helpers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use norn_ipc::Endpoint;
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest, AppendSamplesResponse};
use norn_runtime::{Health, RunReport};
use norn_server::{Daemon, DaemonConfig, DaemonError};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::codegen::tokio_stream::Stream as TokioStream;
use tonic::transport::Channel;

pub fn temp(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("norn-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub struct Running {
    pub channel: Channel,
    pub cancel: CancellationToken,
    pub task: JoinHandle<Result<RunReport, DaemonError>>,
    pub data: PathBuf,
}

pub async fn start(config: DaemonConfig, data: PathBuf) -> Running {
    let listen: Endpoint = "tcp:127.0.0.1:0".parse().unwrap();
    let daemon = Daemon::bind(config.listen(listen).data_dir(data.clone()))
        .await
        .unwrap();
    let endpoint = daemon.local_endpoint().clone();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    health.reached(Health::Serving).await;
    let channel = norn_ipc::connect(&endpoint).await.unwrap();
    Running {
        channel,
        cancel,
        task,
        data,
    }
}

impl Running {
    /// Stops the daemon and removes its data: a test that wants the directory
    /// afterwards calls [`Running::stop_keeping`], and then owns removing it.
    pub async fn stop(self) {
        let data = self.stop_keeping().await;
        let _ = std::fs::remove_dir_all(data);
    }

    pub async fn stop_keeping(self) -> PathBuf {
        self.cancel.cancel();
        self.task.await.unwrap().unwrap();
        self.data
    }
}

/// `streams/plant`, `entities/dev`, and `entities/dev/signals/{id}` for `kinds`.
pub async fn seed(channel: &Channel, kinds: &[(&str, ValueType)]) {
    let stream = CreateStreamRequest {
        stream_id: "plant".into(),
        stream: Some(Stream::default()),
        validate_only: false,
    };
    StreamServiceClient::new(channel.clone())
        .create_stream(stream)
        .await
        .unwrap();
    let entity = CreateEntityRequest {
        entity_id: "dev".into(),
        entity: Some(Entity::default()),
        validate_only: false,
    };
    EntityServiceClient::new(channel.clone())
        .create_entity(entity)
        .await
        .unwrap();
    for (id, kind) in kinds {
        let signal = CreateSignalRequest {
            parent: "entities/dev".into(),
            signal_id: (*id).into(),
            signal: Some(Signal {
                value_type: *kind as i32,
                stream: "streams/plant".into(),
                ..Signal::default()
            }),
            validate_only: false,
        };
        SignalServiceClient::new(channel.clone())
            .create_signal(signal)
            .await
            .unwrap();
    }
}

pub fn double(signal: &str, sequence: i64) -> Sample {
    Sample {
        signal: format!("entities/dev/signals/{signal}"),
        observe_time: Some(prost_types::Timestamp {
            seconds: 1_000 + sequence,
            nanos: 0,
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

pub fn batch(id: i64, samples: Vec<Sample>, level: AcknowledgementLevel) -> AppendSamplesRequest {
    AppendSamplesRequest {
        stream: "streams/plant".into(),
        batch_id: id,
        samples,
        ack_level: level as i32,
    }
}

pub struct Requests(pub mpsc::Receiver<AppendSamplesRequest>);

impl TokioStream for Requests {
    type Item = AppendSamplesRequest;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// Sends `batches` on one call, closes it, and returns every response.
pub async fn append(
    channel: &Channel,
    batches: Vec<AppendSamplesRequest>,
) -> Vec<AppendSamplesResponse> {
    let (tx, rx) = mpsc::channel(batches.len().max(1));
    for b in batches {
        tx.send(b).await.unwrap();
    }
    drop(tx);
    let mut client = TelemetryServiceClient::new(channel.clone());
    let mut replies = client
        .append_samples(Requests(rx))
        .await
        .unwrap()
        .into_inner();
    let mut out = Vec::new();
    while let Some(reply) = replies.message().await.unwrap() {
        out.push(reply);
    }
    out
}

/// The stream directory's samples, as read back by storage, with the signal as
/// a raw compact ID. Clients never do this; a test must see what is on disk.
pub fn stored(data: &std::path::Path) -> Vec<norn_core::Sample> {
    let opts = norn_storage::StorageOptions::default();
    let mut out = Vec::new();
    let Ok(streams) = std::fs::read_dir(data.join("streams")) else {
        return out;
    };
    for dir in streams {
        for segment in norn_storage::live_segments(&dir.unwrap().path()).unwrap() {
            let mut reader = norn_storage::SegmentReader::open(&segment.path, opts).unwrap();
            let all = reader.samples(None, norn_core::Timestamp::MIN, norn_core::Timestamp::MAX);
            out.extend(all.map(Result::unwrap));
        }
    }
    out
}
