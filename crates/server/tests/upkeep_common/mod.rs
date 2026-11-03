//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Long-running operations the way a client uses them, and comparing answers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use norn_core::{Config, MemoryBudgets, StorageConfig};
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{GetServerRequest, Server};
use norn_protocol::google::longrunning::operations_client::OperationsClient;
use norn_protocol::google::longrunning::{GetOperationRequest, Operation, operation};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{
    ApplyStreamRetentionRequest, ApplyStreamRetentionResponse, CompactStreamRequest,
    CompactStreamResponse, GetStreamRequest, Stream, StreamStats, VerifyStreamRequest,
    VerifyStreamResponse,
};
use tonic::transport::Channel;

pub const PLANT: &str = "streams/plant";

/// A daemon whose ingest rolls segments every few hundred chunks and stores a
/// chunk within milliseconds, so a test sees many sealed segments quickly.
pub fn rolling() -> Config {
    let mut memory = MemoryBudgets::default();
    memory.index_bytes = 128 << 10;
    let mut storage = StorageConfig::default();
    storage.max_chunk_age = Duration::from_millis(10);
    Config::builder()
        .memory(memory)
        .storage(storage)
        .build()
        .unwrap()
}

pub async fn finished(channel: &Channel, op: Operation) -> Operation {
    let mut client = OperationsClient::new(channel.clone());
    let mut op = op;
    while !op.done {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let name = op.name.clone();
        let request = GetOperationRequest { name };
        op = client.get_operation(request).await.unwrap().into_inner();
    }
    op
}

fn response<M: prost::Name + Default>(op: Operation) -> M {
    match op.result {
        Some(operation::Result::Response(any)) => any.to_msg().unwrap(),
        other => panic!("{} did not succeed: {other:?}", op.name),
    }
}

pub async fn compact(channel: &Channel) -> CompactStreamResponse {
    let request = CompactStreamRequest {
        name: PLANT.into(),
        interval: None,
    };
    let mut client = StreamServiceClient::new(channel.clone());
    let op = client.compact_stream(request).await.unwrap().into_inner();
    response(finished(channel, op).await)
}

pub async fn verify(channel: &Channel) -> VerifyStreamResponse {
    let name = PLANT.into();
    let mut client = StreamServiceClient::new(channel.clone());
    let op = client.verify_stream(VerifyStreamRequest { name }).await;
    response(finished(channel, op.unwrap().into_inner()).await)
}

pub async fn retention(channel: &Channel, validate_only: bool) -> ApplyStreamRetentionResponse {
    let request = ApplyStreamRetentionRequest {
        name: PLANT.into(),
        validate_only,
    };
    let mut client = StreamServiceClient::new(channel.clone());
    let op = client.apply_stream_retention(request).await;
    response(finished(channel, op.unwrap().into_inner()).await)
}

pub async fn stream(channel: &Channel) -> Stream {
    let name = PLANT.into();
    let mut client = StreamServiceClient::new(channel.clone());
    let got = client.get_stream(GetStreamRequest { name }).await;
    got.unwrap().into_inner()
}

pub async fn stats(channel: &Channel) -> StreamStats {
    stream(channel).await.stats.unwrap()
}

pub async fn server(channel: &Channel) -> Server {
    let name = "server".into();
    let mut client = AdminServiceClient::new(channel.clone());
    let got = client.get_server(GetServerRequest { name }).await;
    got.unwrap().into_inner()
}

/// The sealed segment files of the only stream, by listing.
pub fn segments(data: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut streams = std::fs::read_dir(data.join("streams")).unwrap();
    let dir = streams.next().unwrap().unwrap().path();
    let live = norn_storage::live_segments(&dir).unwrap();
    live.into_iter().map(|s| s.path).collect()
}
