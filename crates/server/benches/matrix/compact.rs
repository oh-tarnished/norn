//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `CompactStream` over the whole stream, polled to the end, as `nornctl compact`
//! does: the time it takes is the time a caller waits.

use std::time::{Duration, Instant};

use norn_protocol::google::longrunning::operations_client::OperationsClient;
use norn_protocol::google::longrunning::{GetOperationRequest, operation};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CompactStreamRequest, CompactStreamResponse};
use tonic::transport::Channel;

use crate::seed::STREAM;

/// How often the operation is polled; this bounds the time's resolution.
const POLL: Duration = Duration::from_millis(20);

/// Compacts the stream; returns what changed and the seconds it took.
pub async fn compact(channel: &Channel) -> (CompactStreamResponse, f64) {
    let began = Instant::now();
    let request = CompactStreamRequest {
        name: STREAM.into(),
        interval: None,
    };
    let mut streams = StreamServiceClient::new(channel.clone());
    let mut op = streams.compact_stream(request).await.unwrap().into_inner();
    let mut operations = OperationsClient::new(channel.clone());
    while !op.done {
        tokio::time::sleep(POLL).await;
        let name = op.name.clone();
        let polled = operations.get_operation(GetOperationRequest { name }).await;
        op = polled.unwrap().into_inner();
    }
    let took = began.elapsed().as_secs_f64();
    match op.result {
        Some(operation::Result::Response(any)) => (any.to_msg().unwrap(), took),
        other => panic!("compaction failed: {other:?}"),
    }
}
