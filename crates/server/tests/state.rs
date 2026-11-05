//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `ComputeState` over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "query_common/mod.rs"]
mod query_common;

use std::io::{Seek, SeekFrom, Write};

use common::{append, batch, double, seed, start, temp};
use norn_protocol::common::ErrorReason;
use norn_protocol::common::ValueType::Double;
use norn_protocol::state::state_service_client::StateServiceClient;
use norn_protocol::state::{ComputeStateRequest, ComputeStateResponse};
use norn_protocol::telemetry::AcknowledgementLevel::{Durable, Persisted};
use norn_server::DaemonConfig;
use norn_storage::{SegmentReader, StorageOptions, live_segments};
use query_common::reason;
use tonic::Code;
use tonic::transport::Channel;

fn at(seconds: i64) -> ComputeStateRequest {
    ComputeStateRequest {
        parent: "entities/dev".into(),
        state_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
        ..ComputeStateRequest::default()
    }
}

async fn call(c: &Channel, r: ComputeStateRequest) -> Result<ComputeStateResponse, tonic::Status> {
    let mut client = StateServiceClient::new(c.clone());
    client.compute_state(r).await.map(|r| r.into_inner())
}

fn sequences(r: &ComputeStateResponse) -> Vec<Option<i64>> {
    let of = |s: &norn_protocol::state::SignalState| s.sample.as_ref().map(|x| x.sequence);
    r.signal_states.iter().map(of).collect()
}

const THREE: [(&str, norn_protocol::common::ValueType); 3] =
    [("a", Double), ("b", Double), ("c", Double)];

#[tokio::test(flavor = "multi_thread")]
async fn a_persisted_sample_is_in_the_state_at_once_and_a_silent_signal_is_unknown() {
    let d = start(DaemonConfig::default(), temp("state-fresh")).await;
    seed(&d.channel, &THREE).await;
    let list = vec![double("a", 5), double("a", 9), double("b", 7)];
    append(&d.channel, vec![batch(1, list, Persisted)]).await;
    let got = call(&d.channel, at(1_008)).await.unwrap();
    assert_eq!(sequences(&got), [Some(5), Some(7), None]);
    assert_eq!(got.state_time.unwrap().seconds, 1_008);
    let names: Vec<_> = got.signal_states.iter().map(|s| &s.signal).collect();
    assert_eq!(names[0], "entities/dev/signals/a", "in signal name order");
    let got = call(&d.channel, at(1_009)).await.unwrap();
    assert_eq!(sequences(&got), [Some(9), Some(7), None]);
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn max_age_turns_an_old_sample_into_unknown() {
    let d = start(DaemonConfig::default(), temp("state-age")).await;
    seed(&d.channel, &THREE).await;
    append(&d.channel, vec![batch(1, vec![double("a", 5)], Persisted)]).await;
    let mut r = at(1_015);
    r.max_age = Some(prost_types::Duration {
        seconds: 10,
        nanos: 0,
    });
    assert_eq!(
        sequences(&call(&d.channel, r.clone()).await.unwrap())[0],
        Some(5)
    );
    r.max_age.as_mut().unwrap().seconds = 9;
    assert_eq!(
        sequences(&call(&d.channel, r.clone()).await.unwrap())[0],
        None
    );
    r.max_age.as_mut().unwrap().seconds = -1;
    let refused = call(&d.channel, r).await.unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_page_describes_one_state_though_late_samples_arrive_between_them() {
    let d = start(DaemonConfig::default(), temp("state-pin")).await;
    seed(&d.channel, &THREE).await;
    let list = vec![double("a", 1), double("b", 2), double("c", 3)];
    append(&d.channel, vec![batch(1, list, Persisted)]).await;
    let mut r = at(2_000);
    r.page_size = 1;
    let first = call(&d.channel, r.clone()).await.unwrap();
    assert_eq!(sequences(&first), [Some(1)]);
    append(&d.channel, vec![batch(1, vec![double("b", 90)], Persisted)]).await;
    r.page_token = first.next_page_token;
    let second = call(&d.channel, r.clone()).await.unwrap();
    assert_eq!(sequences(&second), [Some(2)], "the pin holds");
    r.page_token = second.next_page_token;
    r.page_size = 10;
    let third = call(&d.channel, r.clone()).await.unwrap();
    assert_eq!(sequences(&third), [Some(3)]);
    assert!(third.next_page_token.is_empty());
    let fresh = call(&d.channel, at(2_000)).await.unwrap();
    assert_eq!(sequences(&fresh)[1], Some(90), "a new call sees it");
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_for_another_request_or_a_finished_one_is_refused_by_reason() {
    let d = start(DaemonConfig::default(), temp("state-token")).await;
    seed(&d.channel, &THREE).await;
    let mut r = at(2_000);
    r.page_size = 2;
    let first = call(&d.channel, r.clone()).await.unwrap();
    let mut other = at(2_001);
    other.page_token = first.next_page_token.clone();
    let refused = call(&d.channel, other).await.unwrap_err();
    assert_eq!(reason(&refused), ErrorReason::InvalidToken.as_str_name());
    r.page_token = first.next_page_token;
    call(&d.channel, r.clone()).await.unwrap();
    let refused = call(&d.channel, r).await.unwrap_err();
    assert_eq!(reason(&refused), ErrorReason::InvalidToken.as_str_name());
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_with_no_instant_or_an_unknown_entity_is_refused() {
    let d = start(DaemonConfig::default(), temp("state-bad")).await;
    let mut r = at(1);
    r.state_time = None;
    let refused = call(&d.channel, r).await.unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    let refused = call(&d.channel, at(1)).await.unwrap_err();
    assert_eq!(refused.code(), Code::NotFound);
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupt_chunk_is_data_loss_and_never_an_unknown_signal() {
    let d = start(DaemonConfig::default(), temp("state-corrupt")).await;
    seed(&d.channel, &[("a", Double)]).await;
    let list = (0..50).map(|n| double("a", n)).collect();
    append(&d.channel, vec![batch(1, list, Durable)]).await;
    let data = d.stop_keeping().await;
    let stream = std::fs::read_dir(data.join("streams"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let segment = live_segments(&stream.path()).unwrap().remove(0).path;
    let reader = SegmentReader::open(&segment, StorageOptions::default()).unwrap();
    let chunk = reader.chunks()[0];
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&segment)
        .unwrap();
    let end = chunk.offset + chunk.frame_len() - 1;
    file.seek(SeekFrom::Start(end)).unwrap();
    file.write_all(&[0x5a]).unwrap();
    drop(file);

    let d = start(DaemonConfig::default(), data).await;
    let refused = call(&d.channel, at(2_000)).await.unwrap_err();
    assert_eq!(refused.code(), Code::DataLoss);
    assert_eq!(reason(&refused), ErrorReason::DataCorrupted.as_str_name());
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}
