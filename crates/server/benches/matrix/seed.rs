//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Creating the catalogue a row needs: one stream, one entity, and its signals
//! a thousand to a `BatchCreateSignals` call, so 100,000 signals take a hundred
//! calls rather than a hundred thousand.

use std::sync::Arc;

use norn_protocol::common::ValueType;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{BatchCreateSignalsRequest, CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use tonic::transport::Channel;

/// The stream every row writes.
pub const STREAM: &str = "streams/dev";

/// The most `BatchCreateSignals` takes at once (protocol 1.6).
const BATCH: usize = 1_000;

fn signal(n: usize) -> CreateSignalRequest {
    CreateSignalRequest {
        parent: "entities/dev".into(),
        signal_id: format!("s{n}"),
        signal: Some(Signal {
            value_type: ValueType::Double as i32,
            stream: STREAM.into(),
            ..Signal::default()
        }),
        validate_only: false,
    }
}

/// Creates `signals` signals `s0`.. under `entities/dev`; returns their names.
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
        entity_id: "dev".into(),
        entity: Some(Entity::default()),
        validate_only: false,
    };
    EntityServiceClient::new(channel.clone())
        .create_entity(entity)
        .await
        .unwrap();
    let mut client = SignalServiceClient::new(channel.clone());
    for from in (0..signals).step_by(BATCH) {
        let to = (from + BATCH).min(signals);
        let request = BatchCreateSignalsRequest {
            parent: "entities/dev".into(),
            requests: (from..to).map(signal).collect(),
            validate_only: false,
        };
        let made = client.batch_create_signals(request).await.unwrap();
        assert_eq!(made.into_inner().signals.len(), to - from);
    }
    Arc::new(
        (0..signals)
            .map(|n| format!("entities/dev/signals/s{n}"))
            .collect(),
    )
}
