//! Refused deletes and immutable fields, over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{append, batch, double, seed, start, temp};
use norn_protocol::common::ValueType;
use norn_protocol::entity::DeleteEntityRequest;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::rpc::StatusExt;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::*;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::*;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_server::DaemonConfig;
use prost_types::FieldMask;
use tonic::Code;

fn reason(status: &tonic::Status) -> String {
    status
        .get_details_error_info()
        .expect("exactly one ErrorInfo")
        .reason
}

#[tokio::test(flavor = "multi_thread")]
async fn deletes_refuse_anything_holding_children_or_samples() {
    let d = start(DaemonConfig::default(), temp("catalog")).await;
    seed(
        &d.channel,
        &[("t", ValueType::Double), ("idle", ValueType::Bool)],
    )
    .await;
    append(&d.channel, vec![batch(1, vec![double("t", 1)], Durable)]).await;
    let held = |s: &tonic::Status| {
        assert_eq!(s.code(), Code::FailedPrecondition);
        assert_eq!(reason(s), "ERROR_REASON_RESOURCE_NOT_EMPTY");
    };
    let mut signals = SignalServiceClient::new(d.channel.clone());
    let del = |name: &str| DeleteSignalRequest {
        name: name.into(),
        ..Default::default()
    };
    held(
        &signals
            .delete_signal(del("entities/ahu/signals/t"))
            .await
            .unwrap_err(),
    );
    signals
        .delete_signal(del("entities/ahu/signals/idle"))
        .await
        .unwrap();
    let mut entities = EntityServiceClient::new(d.channel.clone());
    let e = DeleteEntityRequest {
        name: "entities/ahu".into(),
        ..Default::default()
    };
    held(&entities.delete_entity(e).await.unwrap_err());
    let mut streams = StreamServiceClient::new(d.channel.clone());
    let s = DeleteStreamRequest {
        name: "streams/plant".into(),
        ..Default::default()
    };
    held(&streams.delete_stream(s).await.unwrap_err());

    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::default(), data).await;
    let mut signals = SignalServiceClient::new(d.channel.clone());
    let refused = signals
        .delete_signal(del("entities/ahu/signals/t"))
        .await
        .unwrap_err();
    held(&refused);
    let mut streams = StreamServiceClient::new(d.channel.clone());
    let get = GetStreamRequest {
        name: "streams/plant".into(),
    };
    let stats = streams
        .get_stream(get)
        .await
        .unwrap()
        .into_inner()
        .stats
        .unwrap();
    assert_eq!(stats.sample_count, 1);
    assert!(stats.stored_bytes > 0);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn immutable_fields_cannot_be_masked_and_compaction_is_not_implemented() {
    let d = start(DaemonConfig::default(), temp("catalog")).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let mut signals = SignalServiceClient::new(d.channel.clone());
    let list = ListSignalsRequest {
        parent: "entities/-".into(),
        ..Default::default()
    };
    let listed = signals
        .list_signals(list)
        .await
        .unwrap()
        .into_inner()
        .signals;
    assert_eq!(listed[0].stream, "streams/plant");
    let request = UpdateSignalRequest {
        signal: Some(listed[0].clone()),
        update_mask: Some(FieldMask {
            paths: vec!["value_type".into()],
        }),
        validate_only: false,
    };
    let e = signals.update_signal(request).await.unwrap_err();
    assert_eq!(e.code(), Code::InvalidArgument);
    let mut streams = StreamServiceClient::new(d.channel.clone());
    let e = streams
        .export_stream(ExportStreamRequest::default())
        .await
        .unwrap_err();
    assert_eq!(e.code(), Code::Unimplemented);
    assert_eq!(reason(&e), "ERROR_REASON_NOT_IMPLEMENTED");
    d.stop().await;
}
