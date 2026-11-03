//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A client of Norn with nothing of the daemon in it: `norn-ipc`, `norn-protocol`,
//! `tonic`, `tokio` and `prost-types`. A device adapter that records readings into
//! Norn starts from this; see docs/reference/protocol.md, "Writing a client".
//!
//! `cargo run -p norn-cli --example plain_client -- unix:/path/to/norn.sock`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::pin::Pin;
use std::task::{Context, Poll};

use norn_ipc::{Endpoint, connect};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest};
use tokio::sync::mpsc;
use tonic::Code;
use tonic::codegen::tokio_stream;

/// A bounded channel as the request stream, so a slow daemon stalls the sender.
struct Batches(mpsc::Receiver<AppendSamplesRequest>);

impl tokio_stream::Stream for Batches {
    type Item = AppendSamplesRequest;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// Creating is idempotent for a client: `ALREADY_EXISTS` means a past run did.
fn created<T>(result: Result<T, tonic::Status>) {
    if let Err(status) = result {
        assert_eq!(status.code(), Code::AlreadyExists, "{status}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let text = std::env::args().nth(1).expect("an endpoint");
    let endpoint: Endpoint = text.parse().unwrap();
    let channel = connect(&endpoint).await.unwrap();

    let stream = CreateStreamRequest {
        stream_id: "plant".into(),
        stream: Some(Stream::default()),
        validate_only: false,
    };
    let mut streams = StreamServiceClient::new(channel.clone());
    created(streams.create_stream(stream).await);
    let entity = CreateEntityRequest {
        entity_id: "dev-01".into(),
        entity: Some(Entity::default()),
        validate_only: false,
    };
    let mut entities = EntityServiceClient::new(channel.clone());
    created(entities.create_entity(entity).await);
    let signal = CreateSignalRequest {
        parent: "entities/dev-01".into(),
        signal_id: "temperature".into(),
        signal: Some(Signal {
            value_type: ValueType::Double as i32,
            stream: "streams/plant".into(),
            ..Signal::default()
        }),
        validate_only: false,
    };
    let mut signals = SignalServiceClient::new(channel.clone());
    created(signals.create_signal(signal).await);

    let (tx, rx) = mpsc::channel(16);
    let mut telemetry = TelemetryServiceClient::new(channel);
    let mut replies = telemetry
        .append_samples(Batches(rx))
        .await
        .unwrap()
        .into_inner();
    let seconds = i64::from(std::process::id()) + 1_700_000_000;
    tx.send(AppendSamplesRequest {
        stream: "streams/plant".into(),
        batch_id: 1,
        samples: vec![Sample {
            signal: "entities/dev-01/signals/temperature".into(),
            observe_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
            value: Some(Value {
                kind: Some(Kind::DoubleValue(21.5)),
            }),
            quality: Some(Quality {
                severity: QualitySeverity::Good as i32,
                substatus: 0,
            }),
            sequence: 1,
        }],
        ack_level: AcknowledgementLevel::Durable as i32,
    })
    .await
    .unwrap();
    drop(tx);
    while let Some(reply) = replies.message().await.unwrap() {
        assert!(reply.error.is_none(), "{reply:?}");
        let level = AcknowledgementLevel::try_from(reply.ack_level).unwrap();
        println!("batch {} reached {}", reply.batch_id, level.as_str_name());
    }
}
