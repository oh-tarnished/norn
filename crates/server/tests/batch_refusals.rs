//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Batch creates refused whole, naming the request at fault; bounded; and
//! durable across a restart.
//! safe to repeat, bounded, and refusing a request by its index.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod batch_common;
mod common;

use batch_common::{count, field, signal, signals};
use common::{seed, start, temp};
use norn_protocol::common::ValueType;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_server::DaemonConfig;
use tonic::Code;

#[tokio::test(flavor = "multi_thread")]
async fn a_conflicting_or_bad_request_fails_the_whole_batch_and_names_it() {
    let d = start(DaemonConfig::default(), temp("batch")).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let mut client = SignalServiceClient::new(d.channel.clone());
    let new = |i: usize| signal("ahu", &format!("n-{i}"), ValueType::Double);
    let conflict = vec![new(0), signal("ahu", "t", ValueType::Bool)];
    let e = client
        .batch_create_signals(signals("entities/ahu", conflict))
        .await
        .unwrap_err();
    assert_eq!(e.code(), Code::AlreadyExists);
    let bad = vec![new(0), new(1), signal("ahu", "Bad", ValueType::Double)];
    let e = client
        .batch_create_signals(signals("entities/ahu", bad))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), field(&e)),
        (Code::InvalidArgument, "requests[2].signal_id".into())
    );
    let elsewhere = vec![new(0), signal("other", "x", ValueType::Double)];
    let e = client
        .batch_create_signals(signals("entities/ahu", elsewhere))
        .await
        .unwrap_err();
    assert_eq!(field(&e), "requests[1].parent");
    let missing = vec![new(0), signal("nope", "x", ValueType::Double)];
    let e = client
        .batch_create_signals(signals("entities/-", missing))
        .await
        .unwrap_err();
    assert_eq!(e.code(), Code::NotFound);
    let mut nested = new(0);
    nested.validate_only = true;
    let e = client
        .batch_create_signals(signals("entities/ahu", vec![nested]))
        .await
        .unwrap_err();
    assert_eq!(field(&e), "requests[0].validate_only");
    assert_eq!(count(&mut client).await, 1, "nothing was created");
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_or_oversized_batch_is_refused_and_validate_only_stores_nothing() {
    let d = start(DaemonConfig::default(), temp("batch")).await;
    seed(&d.channel, &[]).await;
    let mut client = SignalServiceClient::new(d.channel.clone());
    for n in [0, 1_001] {
        let requests = (0..n)
            .map(|i| signal("ahu", &format!("s-{i}"), ValueType::Double))
            .collect();
        let e = client
            .batch_create_signals(signals("entities/ahu", requests))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code(), field(&e)),
            (Code::InvalidArgument, "requests".into())
        );
    }
    let mut dry = signals("entities/ahu", vec![signal("ahu", "s", ValueType::Double)]);
    dry.validate_only = true;
    let made = client.batch_create_signals(dry).await.unwrap().into_inner();
    assert_eq!(made.signals[0].name, "entities/ahu/signals/s");
    assert_eq!(count(&mut client).await, 0);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_survives_a_restart_whole() {
    let d = start(DaemonConfig::default(), temp("batch")).await;
    seed(&d.channel, &[]).await;
    let requests = (0..1_000)
        .map(|i| signal("ahu", &format!("s-{i}"), ValueType::Double))
        .collect();
    let mut client = SignalServiceClient::new(d.channel.clone());
    let made = client
        .batch_create_signals(signals("entities/ahu", requests))
        .await
        .unwrap()
        .into_inner();
    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::default(), data).await;
    let mut client = SignalServiceClient::new(d.channel.clone());
    assert_eq!(count(&mut client).await, 1_000);
    let get = norn_protocol::signal::GetSignalRequest {
        name: "entities/ahu/signals/s-999".into(),
    };
    let back = client.get_signal(get).await.unwrap().into_inner();
    assert_eq!(back, made.signals[999]);
    d.stop().await;
}
