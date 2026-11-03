//! `BrowseSamples` over real gRPC: every page size reads exactly what the stream
//! reads, bounds hold, and a token serves only its own request.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "query_common/mod.rs"]
mod query_common;
#[path = "reads_common/mod.rs"]
mod reads_common;

use common::{start, temp};
use norn_protocol::common::Sample;
use norn_protocol::common::value::Kind;
use norn_protocol::query::{
    Aggregate, AggregateFunction, Aggregation, BrowseSamplesRequest, QuerySamplesRequest,
    SampleOrder,
};
use norn_server::DaemonConfig;
use query_common::{aggregates, reason, samples};
use reads_common::{MODE, TEMP, query, seed};
use tonic::Code;
use tonic::transport::Channel;

fn request(order: SampleOrder) -> BrowseSamplesRequest {
    BrowseSamplesRequest {
        parent: "entities/-".into(),
        order: order as i32,
        max_value_bytes: 65_536,
        ..BrowseSamplesRequest::default()
    }
}

/// Every page of `r`, at `page_size`.
async fn browse_all(
    c: &Channel,
    mut r: BrowseSamplesRequest,
    size: i32,
) -> (Vec<Sample>, Vec<Aggregate>) {
    r.page_size = size;
    let (mut s, mut a, mut calls) = (Vec::new(), Vec::new(), 0);
    loop {
        let page = query(c)
            .browse_samples(r.clone())
            .await
            .unwrap()
            .into_inner();
        assert!(page.samples.len() + page.aggregates.len() <= size as usize);
        s.extend(page.samples);
        a.extend(page.aggregates);
        calls += 1;
        assert!(calls < 2_000, "a browse never ended");
        if page.next_page_token.is_empty() {
            return (s, a);
        }
        r.page_token = page.next_page_token;
    }
}

fn streamed(order: SampleOrder) -> QuerySamplesRequest {
    QuerySamplesRequest {
        parent: "entities/-".into(),
        order: order as i32,
        ..QuerySamplesRequest::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_page_size_reads_exactly_what_the_stream_reads_in_both_orders() {
    let d = start(DaemonConfig::default(), temp("browse-pages")).await;
    seed(&d.channel).await;
    for order in [SampleOrder::BySignal, SampleOrder::ByTime] {
        let want = samples(&d.channel, streamed(order)).await;
        assert_eq!(want.len(), 1_100);
        for size in [1, 7, 333, 1000] {
            let (got, _) = browse_all(&d.channel, request(order), size).await;
            assert_eq!(got, want, "{order:?} at page size {size}");
        }
    }
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn windowed_aggregates_and_latest_page_like_the_stream() {
    let d = start(DaemonConfig::default(), temp("browse-agg")).await;
    seed(&d.channel).await;
    for (function, window) in [
        (AggregateFunction::Count, Some(7)),
        (AggregateFunction::Latest, None),
    ] {
        let aggregation = Some(Aggregation {
            aggregate_function: function as i32,
            window: window.map(|seconds| prost_types::Duration { seconds, nanos: 0 }),
        });
        let mut stream = streamed(SampleOrder::BySignal);
        stream.aggregation = aggregation;
        stream.interval = Some(norn_protocol::google::r#type::Interval {
            start_time: reads_common::at(1_003),
            end_time: None,
        });
        let want = aggregates(&d.channel, stream.clone()).await;
        assert!(!want.is_empty());
        let mut r = request(SampleOrder::BySignal);
        (r.aggregation, r.interval) = (aggregation, stream.interval);
        for size in [1, 3, 1000] {
            let (_, got) = browse_all(&d.channel, r.clone(), size).await;
            assert_eq!(got, want, "{function:?} at page size {size}");
        }
    }
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn defaults_are_bounded_and_a_long_value_is_cut_and_flagged() {
    let d = start(DaemonConfig::default(), temp("browse-bounds")).await;
    seed(&d.channel).await;
    let r = BrowseSamplesRequest {
        parent: "entities/-".into(),
        ..BrowseSamplesRequest::default()
    };
    let page = query(&d.channel)
        .browse_samples(r)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(page.samples.len(), 100, "the default page");
    assert!(!page.next_page_token.is_empty());
    let r = BrowseSamplesRequest {
        parent: "entities/ahu".into(),
        filter: "id = \"mode\"".into(),
        page_size: 1000,
        ..BrowseSamplesRequest::default()
    };
    let page = query(&d.channel)
        .browse_samples(r)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(page.truncated_indexes, [50]);
    let cut = &page.samples[50];
    assert_eq!(cut.signal, MODE);
    let Some(Kind::StringValue(text)) = cut.value.as_ref().unwrap().kind.as_ref() else {
        panic!()
    };
    assert_eq!(text.len(), 1_024);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_serves_only_its_own_request_and_bad_fields_are_refused() {
    let d = start(DaemonConfig::default(), temp("browse-token")).await;
    seed(&d.channel).await;
    let mut r = request(SampleOrder::BySignal);
    r.page_size = 5;
    let page = query(&d.channel)
        .browse_samples(r.clone())
        .await
        .unwrap()
        .into_inner();
    let mut other = r.clone();
    (other.filter, other.page_token) = (format!("id = \"{}\"", "temp"), page.next_page_token);
    let refused = query(&d.channel).browse_samples(other).await.unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert_eq!(reason(&refused), "ERROR_REASON_INVALID_TOKEN");
    r.page_size = -1;
    let refused = query(&d.channel).browse_samples(r).await.unwrap_err();
    assert_eq!(reason(&refused), "ERROR_REASON_INVALID_ARGUMENT");
    assert!(TEMP.starts_with("entities/ahu"));
    d.stop().await;
}
