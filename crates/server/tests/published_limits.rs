//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The size limits `Limits` publishes are the ones the daemon enforces: a
//! message of exactly the limit passes the size check, one byte more is refused
//! before it is decoded, on unary, server-streaming and `AppendSamples` calls.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Requests, start, temp};
use norn_core::{Config, MemoryBudgets};
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{GetServerRequest, Limits};
use norn_protocol::rpc::StatusExt;
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AppendSamplesRequest, SubscribeEventsRequest};
use norn_server::{DaemonConfig, MAX_REQUEST_BYTES};
use prost::Message;
use tonic::Code;
use tonic::transport::Channel;

async fn limits(channel: &Channel) -> Limits {
    let request = GetServerRequest {
        name: "server".into(),
    };
    let mut admin = AdminServiceClient::new(channel.clone());
    let server = admin.get_server(request).await.unwrap();
    server.into_inner().limits.unwrap()
}

/// A string of the length that makes `make(string)` encode to exactly
/// `target` bytes; the length prefix's own width makes this a search.
fn sized<M: Message>(target: usize, make: impl Fn(String) -> M) -> M {
    let mut n = target;
    loop {
        let message = make("x".repeat(n));
        let len = message.encoded_len();
        if len == target {
            return message;
        }
        assert!(len > target && n > 0, "no padding reaches {target} bytes");
        n -= len - target;
    }
}

/// The status is the size refusal: `RESOURCE_EXHAUSTED`, reason
/// `ERROR_REASON_LIMIT_EXCEEDED`, naming `limit`.
fn assert_refused_at(status: &tonic::Status, limit: i64) {
    assert_eq!(status.code(), Code::ResourceExhausted, "{status:?}");
    let info = status.get_details_error_info().unwrap();
    assert_eq!(info.reason, "ERROR_REASON_LIMIT_EXCEEDED");
    assert_eq!(info.metadata["limit"], limit.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn every_size_limit_is_published_and_equals_what_the_daemon_enforces() {
    let d = start(DaemonConfig::default(), temp("published")).await;
    let l = limits(&d.channel).await;
    assert_eq!(l.max_message_bytes, MAX_REQUEST_BYTES as i64);
    assert_eq!(l.max_batch_create_requests, 1000);
    assert!(l.max_batch_bytes > l.max_message_bytes);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_unary_request_one_byte_past_max_message_bytes_is_refused_and_one_at_it_is_not() {
    let d = start(DaemonConfig::default(), temp("published")).await;
    let limit = limits(&d.channel).await.max_message_bytes;
    let target = usize::try_from(limit).unwrap();
    let mut streams = StreamServiceClient::new(d.channel.clone());
    let over = sized(target + 1, |name| GetStreamRequest { name });
    let refused = streams.get_stream(over).await.unwrap_err();
    assert_refused_at(&refused, limit);
    let at = sized(target, |name| GetStreamRequest { name });
    let answered = streams.get_stream(at).await.unwrap_err();
    assert_eq!(answered.code(), Code::InvalidArgument, "{answered:?}");
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streaming_request_one_byte_past_max_message_bytes_is_refused() {
    let d = start(DaemonConfig::default(), temp("published")).await;
    let limit = limits(&d.channel).await.max_message_bytes;
    let target = usize::try_from(limit).unwrap() + 1;
    let over = sized(target, |parent| SubscribeEventsRequest {
        mode: 1,
        parent,
        ..SubscribeEventsRequest::default()
    });
    let mut telemetry = TelemetryServiceClient::new(d.channel.clone());
    let refused = match telemetry.subscribe_events(over).await {
        Err(status) => status,
        Ok(events) => events.into_inner().message().await.unwrap_err(),
    };
    assert_refused_at(&refused, limit);
    d.stop().await;
}

/// Sends `batch` alone on an `AppendSamples` call: the call's failure, or the
/// batch's own answer.
async fn append_one(
    channel: &Channel,
    batch: AppendSamplesRequest,
) -> Result<norn_protocol::telemetry::AppendSamplesResponse, tonic::Status> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(batch).await.unwrap();
    drop(tx);
    let mut client = TelemetryServiceClient::new(channel.clone());
    let mut replies = client.append_samples(Requests(rx)).await?.into_inner();
    replies
        .message()
        .await?
        .ok_or_else(|| tonic::Status::unknown("no reply"))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_append_batch_one_byte_past_max_batch_bytes_is_refused_and_one_at_it_is_not() {
    append_at_the_limit(DaemonConfig::default(), 4 << 20).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_smaller_ipc_budget_publishes_and_enforces_a_smaller_batch_limit() {
    let mut memory = MemoryBudgets::default();
    memory.ipc_bytes = 8 << 20;
    let engine = Config::builder().memory(memory).build().unwrap();
    append_at_the_limit(DaemonConfig::new(engine), 1 << 20).await;
}

/// `config`'s daemon publishes `expected` as `max_batch_bytes`, and enforces it.
async fn append_at_the_limit(config: DaemonConfig, expected: i64) {
    let d = start(config, temp("published")).await;
    let limit = limits(&d.channel).await.max_batch_bytes;
    assert_eq!(limit, expected);
    let target = usize::try_from(limit).unwrap();
    let batch = |stream| AppendSamplesRequest {
        stream,
        batch_id: 1,
        ..AppendSamplesRequest::default()
    };
    let refused = append_one(&d.channel, sized(target + 1, batch)).await;
    assert_refused_at(&refused.unwrap_err(), limit);
    let answered = append_one(&d.channel, sized(target, batch)).await.unwrap();
    let error = answered.error.expect("a batch naming no stream is refused");
    assert_ne!(error.code, Code::ResourceExhausted as i32, "{error:?}");
    d.stop().await;
}
