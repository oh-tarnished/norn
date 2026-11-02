//! Reading from Norn with nothing of the daemon in it: `norn-ipc`, `norn-protocol`,
//! `tonic`, `tokio` and `prost-types`. See docs/guides/querying.md.
//!
//! Prints the last hour of an existing signal's average per minute, and resumes a
//! raw read with its token, the way a client recovers from a broken stream.
//!
//! `cargo run -p norn-cli --example query_client -- unix:/path/to/norn.sock entities/ahu-01 supply-temp`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use norn_ipc::{Endpoint, connect};
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let endpoint: Endpoint = args[1].parse().unwrap();
    let channel = connect(&endpoint).await.unwrap();
    let mut client = QueryServiceClient::new(channel);
    let select = QuerySamplesRequest {
        parent: args[2].clone(),
        filter: format!("id = \"{}\"", args[3]),
        ..QuerySamplesRequest::default()
    };

    let average = QuerySamplesRequest {
        aggregation: Some(Aggregation {
            aggregate_function: AggregateFunction::Average as i32,
            window: Some(prost_types::Duration {
                seconds: 60,
                nanos: 0,
            }),
        }),
        ..select.clone()
    };
    let mut windows = client.query_samples(average).await.unwrap().into_inner();
    while let Some(page) = windows.message().await.unwrap() {
        for a in page.aggregates {
            println!("{} {:?} over {} samples", a.signal, a.value, a.sample_count);
        }
    }

    // Read one response, drop the stream as if it broke, and go on from its token.
    let mut raw = client
        .query_samples(select.clone())
        .await
        .unwrap()
        .into_inner();
    let Some(first) = raw.message().await.unwrap() else {
        return;
    };
    drop(raw);
    let mut total = first.samples.len();
    let resume = QuerySamplesRequest {
        resume_token: first.resume_token,
        ..select
    };
    let mut rest = client.query_samples(resume).await.unwrap().into_inner();
    while let Some(page) = rest.message().await.unwrap() {
        total += page.samples.len();
    }
    println!("{total} samples");
}
