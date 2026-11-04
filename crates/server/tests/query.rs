//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `QuerySamples` over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "query_common/mod.rs"]
mod query_common;

use std::time::Duration;

use common::{append, batch, double, seed, start, temp};
use norn_core::{Config, StorageConfig};
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, GetServerRequest};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{ErrorReason, QualitySeverity, QualitySubstatus, ValueType};
use norn_protocol::query::{AggregateFunction, QuerySamplesRequest, SampleOrder};
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_runtime::query::Filter;
use norn_server::DaemonConfig;
use query_common::{aggregates, all, over, pages, reason, samples};
use tonic::Code;

const SIGNALS: [(&str, ValueType); 2] = [("t", ValueType::Double), ("u", ValueType::Double)];

#[tokio::test(flavor = "multi_thread")]
async fn an_append_at_persisted_is_immediately_queryable_and_counted() {
    let d = start(DaemonConfig::default(), temp("query-fresh")).await;
    seed(&d.channel, &SIGNALS).await;
    append(&d.channel, vec![batch(1, vec![double("t", 7)], Persisted)]).await;
    let got = samples(&d.channel, all()).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].signal, "entities/dev/signals/t");
    assert_eq!(
        got[0].value.as_ref().unwrap().kind,
        Some(Kind::DoubleValue(7.0))
    );
    let name = "streams/plant".to_owned();
    let stream = StreamServiceClient::new(d.channel.clone())
        .get_stream(GetStreamRequest { name })
        .await
        .unwrap()
        .into_inner();
    let stats = stream.stats.unwrap();
    assert_eq!((stats.sample_count, stats.buffered_sample_count), (1, 1));
    assert!(stats.newest_sample_time.is_some());
    let data = d.stop_keeping().await;
    std::fs::remove_dir_all(data).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn exactly_once_holds_through_real_flushes_every_few_milliseconds() {
    let mut storage = StorageConfig::default();
    storage.max_chunk_age = Duration::from_millis(20);
    let engine = Config::builder().storage(storage).build().unwrap();
    let d = start(DaemonConfig::new(engine), temp("query-flush")).await;
    seed(&d.channel, &SIGNALS).await;
    for n in 0..60i64 {
        let list = (n * 50..(n + 1) * 50).map(|i| double("t", i)).collect();
        append(&d.channel, vec![batch(1, list, Persisted)]).await;
        let got = samples(&d.channel, all()).await;
        let seqs: Vec<i64> = got.iter().map(|s| s.sequence).collect();
        assert_eq!(
            seqs,
            (0..(n + 1) * 50).collect::<Vec<_>>(),
            "after batch {n}"
        );
    }
    let data = d.stop_keeping().await;
    std::fs::remove_dir_all(data).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn aggregates_filters_tokens_and_refusals_reach_the_client_typed() {
    let d = start(DaemonConfig::default(), temp("query-api")).await;
    seed(&d.channel, &SIGNALS).await;
    let list = (0..25_000).map(|i| double("t", i)).collect::<Vec<_>>();
    for (n, part) in list.chunks(5_000).enumerate() {
        let id = i64::try_from(n).unwrap() + 1;
        append(&d.channel, vec![batch(id, part.to_vec(), Persisted)]).await;
    }
    let c = &d.channel;

    let counts = aggregates(c, over(AggregateFunction::Count, 10_000)).await;
    let total: i64 = counts.iter().map(|a| a.sample_count).sum();
    assert_eq!((counts.len(), total), (3, 25_000));

    let mut time = all();
    time.order = SampleOrder::ByTime as i32;
    time.sample_filter = "value >= 24999 AND quality.severity = QUALITY_SEVERITY_GOOD".into();
    assert_eq!(samples(c, time).await.len(), 1);

    let whole = pages(c, all()).await.unwrap();
    assert!(whole.len() >= 3, "pages are bounded: {}", whole.len());
    assert!(whole.iter().all(|p| p.samples.len() <= 10_000));
    let resume = QuerySamplesRequest {
        resume_token: whole[0].resume_token.clone(),
        ..all()
    };
    assert_eq!(samples(c, resume).await.len(), 15_000);

    let other = QuerySamplesRequest {
        resume_token: whole[0].resume_token.clone(),
        sample_filter: "value > 1".into(),
        ..all()
    };
    let refused = pages(c, other).await.unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert_eq!(reason(&refused), ErrorReason::InvalidToken.as_str_name());

    let bad = QuerySamplesRequest {
        sample_filter: "nonsense".into(),
        ..all()
    };
    let refused = pages(c, bad).await.unwrap_err();
    assert_eq!(reason(&refused), ErrorReason::InvalidArgument.as_str_name());

    let absent = QuerySamplesRequest {
        parent: "entities/none".into(),
        ..all()
    };
    assert_eq!(pages(c, absent).await.unwrap_err().code(), Code::NotFound);

    let one = QuerySamplesRequest {
        filter: "id = \"u\"".into(),
        ..all()
    };
    assert!(samples(c, one).await.is_empty());

    let server = AdminServiceClient::new(c.clone())
        .get_server(GetServerRequest {
            name: "server".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(
        server
            .capabilities
            .contains(&(Capability::QueryRange as i32))
    );
    assert!(
        server
            .capabilities
            .contains(&(Capability::QueryAggregates as i32))
    );
    let data = d.stop_keeping().await;
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn the_filter_language_knows_every_quality_name_the_protocol_has() {
    let mut words: Vec<String> = Vec::new();
    for n in 1..4 {
        words.push(QualitySeverity::try_from(n).unwrap().as_str_name().into());
    }
    for n in 0.. {
        let Ok(s) = QualitySubstatus::try_from(n) else {
            break;
        };
        words.push(s.as_str_name().into());
    }
    for word in &words {
        let field = word
            .strip_prefix("QUALITY_SEVERITY_")
            .map_or("substatus", |_| "severity");
        let text = format!("quality.{field} = {word}");
        assert!(Filter::parse(&text).is_ok(), "{text}");
    }
    assert_eq!(words.len(), 15);
}
