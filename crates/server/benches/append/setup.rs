//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Creating the signals, and watching the daemon's memory.

use std::sync::Arc;
use std::time::Duration;

use norn_protocol::common::ValueType;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

/// `signals` signals in one stream, created 64 at a time, sharing fsyncs.
pub async fn seed(channel: &Channel, signals: usize) -> Arc<Vec<String>> {
    let stream = CreateStreamRequest {
        stream_id: "dev".into(),
        stream: Some(Stream::default()),
        validate_only: false,
    };
    StreamServiceClient::new(channel.clone())
        .create_stream(stream)
        .await
        .unwrap();
    let entity = CreateEntityRequest {
        entity_id: "site".into(),
        entity: Some(Entity::default()),
        validate_only: false,
    };
    EntityServiceClient::new(channel.clone())
        .create_entity(entity)
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..signals {
        if tasks.len() >= 64 {
            tasks.join_next().await.unwrap().unwrap();
        }
        let client = channel.clone();
        tasks.spawn(async move {
            let request = CreateSignalRequest {
                parent: "entities/site".into(),
                signal_id: format!("s{n}"),
                signal: Some(Signal {
                    value_type: ValueType::Double as i32,
                    stream: "streams/dev".into(),
                    ..Signal::default()
                }),
                validate_only: false,
            };
            SignalServiceClient::new(client)
                .create_signal(request)
                .await
                .unwrap();
        });
    }
    while tasks.join_next().await.is_some() {}
    Arc::new(
        (0..signals)
            .map(|n| format!("entities/site/signals/s{n}"))
            .collect(),
    )
}

/// Samples the daemon's RSS every 100 ms; resolves to the peak, in KiB.
pub fn watch_rss(pid: u32) -> (JoinHandle<u64>, CancellationToken) {
    let stop = CancellationToken::new();
    let token = stop.clone();
    let task = tokio::spawn(async move {
        let mut peak = 0;
        while !token.is_cancelled() {
            let out = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            peak = peak.max(text.trim().parse::<u64>().unwrap_or(0));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        peak
    });
    (task, stop)
}
