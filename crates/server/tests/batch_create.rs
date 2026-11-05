//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `BatchCreateEntities` and `BatchCreateSignals` over real gRPC: a batch
//! across entities, and repeating one.
//! safe to repeat, bounded, and refusing a request by its index.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod batch_common;
mod common;

use batch_common::{count, entity, signal, signals};
use common::{seed, start, temp};
use norn_protocol::common::ValueType;
use norn_protocol::entity::BatchCreateEntitiesRequest;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_server::DaemonConfig;

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_across_entities_creates_every_signal_in_request_order() {
    let d = start(DaemonConfig::default(), temp("batch")).await;
    seed(&d.channel, &[]).await;
    let mut entities = EntityServiceClient::new(d.channel.clone());
    let made = entities
        .batch_create_entities(BatchCreateEntitiesRequest {
            requests: (0..3).map(|i| entity(&format!("dev-{i}"))).collect(),
            validate_only: false,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(made.entities[2].name, "entities/dev-2");
    let mut client = SignalServiceClient::new(d.channel.clone());
    let requests = (0..600)
        .map(|n| {
            signal(
                &format!("dev-{}", n % 3),
                &format!("p-{n}"),
                ValueType::Double,
            )
        })
        .collect();
    let made = client
        .batch_create_signals(signals("entities/-", requests))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(made.signals.len(), 600);
    assert_eq!(made.signals[4].name, "entities/dev-1/signals/p-4");
    assert_eq!(made.signals[4].unit, "degree_celsius");
    assert_eq!(count(&mut client).await, 600);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn repeating_a_batch_returns_the_stored_resources_rather_than_an_error() {
    let d = start(DaemonConfig::default(), temp("batch")).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let mut client = SignalServiceClient::new(d.channel.clone());
    let batch = || {
        let mut r = vec![signal("dev", "t", ValueType::Double)];
        r.extend((0..5).map(|i| signal("dev", &format!("s-{i}"), ValueType::Double)));
        signals("entities/dev", r)
    };
    let first = client
        .batch_create_signals(batch())
        .await
        .unwrap()
        .into_inner();
    let again = client
        .batch_create_signals(batch())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.signals, again.signals, "same names, times and etags");
    assert_eq!(count(&mut client).await, 6);
    let mut entities = EntityServiceClient::new(d.channel.clone());
    let twice = BatchCreateEntitiesRequest {
        requests: vec![entity("dev"), entity("dev-2")],
        validate_only: false,
    };
    let made = entities.batch_create_entities(twice.clone()).await.unwrap();
    let again = entities.batch_create_entities(twice).await.unwrap();
    assert_eq!(made.into_inner(), again.into_inner());
    d.stop().await;
}
