//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Calling `QuerySamples` the way a client does.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use norn_protocol::common::Sample;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{
    Aggregate, AggregateFunction, Aggregation, QuerySamplesRequest, QuerySamplesResponse,
};
use tonic::transport::Channel;

pub fn all() -> QuerySamplesRequest {
    QuerySamplesRequest {
        parent: "entities/dev".into(),
        ..QuerySamplesRequest::default()
    }
}

pub fn over(function: AggregateFunction, seconds: i64) -> QuerySamplesRequest {
    QuerySamplesRequest {
        aggregation: Some(Aggregation {
            aggregate_function: function as i32,
            window: Some(prost_types::Duration { seconds, nanos: 0 }),
        }),
        ..all()
    }
}

pub async fn pages(
    channel: &Channel,
    request: QuerySamplesRequest,
) -> Result<Vec<QuerySamplesResponse>, tonic::Status> {
    let mut client = QueryServiceClient::new(channel.clone());
    let mut stream = client.query_samples(request).await?.into_inner();
    let mut out = Vec::new();
    while let Some(page) = stream.message().await? {
        out.push(page);
    }
    Ok(out)
}

pub async fn samples(channel: &Channel, request: QuerySamplesRequest) -> Vec<Sample> {
    let pages = pages(channel, request).await.unwrap();
    pages.into_iter().flat_map(|p| p.samples).collect()
}

pub async fn aggregates(channel: &Channel, request: QuerySamplesRequest) -> Vec<Aggregate> {
    let pages = pages(channel, request).await.unwrap();
    pages.into_iter().flat_map(|p| p.aggregates).collect()
}

/// The `ErrorInfo` reason in `status`.
pub fn reason(status: &tonic::Status) -> String {
    use norn_protocol::rpc::StatusExt;
    status.get_details_error_info().unwrap().reason
}
