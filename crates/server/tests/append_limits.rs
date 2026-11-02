//! `AppendSamples` at its limits: malformed batches, bytes, samples, the lock.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::time::{Duration, Instant};

use common::{append, batch, double, seed, start, temp};
use norn_core::{Config, Limits, MemoryBudgets};
use norn_protocol::common::ValueType;
use norn_protocol::rpc::pb;
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::{Buffered, Durable, Unspecified};
use norn_protocol::telemetry::{BackpressureOutcome, BackpressureReason};
use norn_server::{Daemon, DaemonConfig};

const SIGNALS: [(&str, ValueType); 2] = [("t", ValueType::Double), ("n", ValueType::Uint32)];

fn outcome(r: &norn_protocol::telemetry::AppendSamplesResponse) -> (i32, i32) {
    let a = r.admission.as_ref().unwrap();
    (a.outcome, a.reason)
}

fn reason_of(status: &pb::Status) -> String {
    use prost::Message;
    let any = status
        .details
        .iter()
        .find(|d| d.type_url.ends_with("ErrorInfo"))
        .unwrap();
    pb::ErrorInfo::decode(&any.value[..]).unwrap().reason
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_past_the_sample_limit_is_rejected_before_prost_sees_its_samples() {
    let mut limits = Limits::default();
    limits.max_batch_samples = 10;
    let engine = Config::builder().limits(limits).build().unwrap();
    let d = start(DaemonConfig::new(engine), temp("append")).await;
    seed(&d.channel, &SIGNALS).await;
    let many = (0..11).map(|n| double("t", n)).collect();
    let replies = append(
        &d.channel,
        vec![
            batch(7, many, Durable),
            batch(8, vec![double("t", 100)], Buffered),
        ],
    )
    .await;
    assert_eq!(replies[0].batch_id, 7);
    assert_eq!(
        outcome(&replies[0]).1,
        BackpressureReason::BatchTooLarge as i32
    );
    assert_eq!(
        reason_of(replies[0].error.as_ref().unwrap()),
        "ERROR_REASON_LIMIT_EXCEEDED"
    );
    assert_eq!(replies[1].batch_id, 8, "the call survives the refusal");
    assert_eq!(outcome(&replies[1]).0, BackpressureOutcome::Accept as i32);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_batches_are_rejected_with_a_reason() {
    let d = start(DaemonConfig::default(), temp("append")).await;
    seed(&d.channel, &SIGNALS).await;
    let mut no_stream = batch(2, vec![double("t", 1)], Buffered);
    no_stream.stream = "streams/ghost".into();
    let replies = append(
        &d.channel,
        vec![
            batch(5, vec![double("t", 1)], Buffered),
            batch(5, vec![double("t", 2)], Buffered),
            batch(6, vec![double("t", 2)], Unspecified),
            batch(7, vec![], Buffered),
        ],
    )
    .await;
    let reasons: Vec<_> = replies[1..]
        .iter()
        .map(|r| reason_of(r.error.as_ref().unwrap()))
        .collect();
    assert_eq!(reasons, ["ERROR_REASON_INVALID_ARGUMENT"; 3]);
    let replies = append(&d.channel, vec![no_stream]).await;
    assert_eq!(
        reason_of(replies[0].error.as_ref().unwrap()),
        "ERROR_REASON_RESOURCE_NOT_FOUND"
    );
    d.stop().await;
}

/// Waits, within a bound, for the stream to count `want` samples: BUFFERED promises
/// nothing about visibility yet, only that they are on their way.
async fn every_sample_arrives(channel: &tonic::transport::Channel, want: i64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let name = "streams/plant".to_owned();
        let stream = StreamServiceClient::new(channel.clone())
            .get_stream(GetStreamRequest { name })
            .await
            .unwrap()
            .into_inner();
        let count = stream.stats.unwrap().sample_count;
        if count == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{count} of {want} samples arrived"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_past_the_byte_limit_fails_the_call_and_many_callers_share_a_tiny_ipc_budget() {
    let mut memory = MemoryBudgets::default();
    memory.ipc_bytes = 1 << 20;
    let engine = Config::builder().memory(memory).build().unwrap();
    let d = start(DaemonConfig::new(engine), temp("append")).await;
    seed(&d.channel, &SIGNALS).await;
    let mut calls = tokio::task::JoinSet::new();
    for c in 0..16 {
        let channel = d.channel.clone();
        calls.spawn(async move {
            let base = c * 1000;
            let samples: Vec<_> = (0..900).map(|n| double("t", base + n)).collect();
            common::append(&channel, vec![batch(1, samples, Buffered)]).await
        });
    }
    // Whether the commit thread drains anything before the last batch is admitted is
    // a race, and either answer is right: `BEHIND_BATCHES` is 16, so only the batch
    // that fills the queue to 16 can hear BUFFER. Nothing is refused or lost.
    let mut behind = 0;
    while let Some(replies) = calls.join_next().await {
        let (kind, reason) = outcome(&replies.unwrap()[0]);
        if kind == BackpressureOutcome::Buffer as i32 {
            assert_eq!(reason, BackpressureReason::WritePathBehind as i32);
            behind += 1;
        } else {
            assert_eq!(kind, BackpressureOutcome::Accept as i32);
        }
    }
    assert!(behind <= 1, "{behind} batches found the queue at 16");
    every_sample_arrives(&d.channel, 16 * 900).await;
    let huge: Vec<_> = (0..4000).map(|n| double("t", 100_000 + n)).collect();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(batch(1, huge, Buffered)).await.unwrap();
    drop(tx);
    let mut client =
        norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient::new(
            d.channel.clone(),
        );
    let mut replies = client
        .append_samples(common::Requests(rx))
        .await
        .unwrap()
        .into_inner();
    let status = replies.message().await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::ResourceExhausted);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_daemon_on_a_database_is_refused_and_the_next_after_it_stops_is_not() {
    let d = start(DaemonConfig::default(), temp("append")).await;
    let listen: norn_ipc::Endpoint = "tcp:127.0.0.1:0".parse().unwrap();
    let second = DaemonConfig::default()
        .listen(listen.clone())
        .data_dir(d.data.clone());
    let refused = Daemon::bind(second).await.unwrap_err();
    assert!(
        refused.to_string().contains("another process holds"),
        "{refused}"
    );
    let data = d.stop_keeping().await;
    start(DaemonConfig::default(), data).await.stop().await;
}
