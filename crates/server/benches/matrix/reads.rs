//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading the history back: latest, range and aggregate queries over the last
//! hour, day and week, StateAt at instants spread over the set, and a replay at
//! maximum speed. One client, closed loop: each answer read whole before the next.

use std::time::Instant;

use norn_protocol::google::r#type::Interval;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest};
use norn_protocol::state::ComputeStateRequest;
use tonic::transport::Channel;

use crate::ingest::EPOCH;
use crate::profile::History;
use crate::replaying::replay;
use crate::stats::{Better, Metrics, percentile};
use crate::timing::{at, state};

/// Every page of one query; returns the samples and aggregates it yielded.
async fn query(channel: &Channel, request: QuerySamplesRequest) -> u64 {
    let mut client = QueryServiceClient::new(channel.clone());
    let mut pages = client.query_samples(request).await.unwrap().into_inner();
    let mut results = 0;
    while let Some(page) = pages.message().await.unwrap() {
        results += (page.samples.len() + page.aggregates.len()) as u64;
    }
    results
}

fn latency(metrics: &mut Metrics, key: &str, mut took_ms: Vec<f64>) {
    took_ms.sort_by(f64::total_cmp);
    let at = |q| percentile(&took_ms, q).unwrap_or(f64::NAN);
    metrics.record(
        &format!("{key}.p50_ms"),
        "ms",
        Better::Lower,
        true,
        at(0.50),
    );
    metrics.record(
        &format!("{key}.p99_ms"),
        "ms",
        Better::Lower,
        false,
        at(0.99),
    );
}

fn request(parent: &str, filter: String, from: i64, to: i64) -> QuerySamplesRequest {
    QuerySamplesRequest {
        parent: parent.into(),
        filter,
        interval: Some(Interval {
            start_time: at(from),
            end_time: at(to),
        }),
        ..QuerySamplesRequest::default()
    }
}

fn aggregate(function: AggregateFunction, window: Option<i64>) -> Option<Aggregation> {
    Some(Aggregation {
        aggregate_function: function as i32,
        window: window.map(|seconds| prost_types::Duration { seconds, nanos: 0 }),
    })
}

/// The three query shapes over every window the history is long enough for.
pub async fn queries(channel: &Channel, h: &History, metrics: &mut Metrics) {
    let end = EPOCH + h.seconds;
    for &(label, window) in &h.windows {
        if window > h.seconds {
            println!("history: the {label} query window is longer than the history; skipped");
            continue;
        }
        let from = end - window;
        let shapes = ["range", "aggregate", "latest"];
        for shape in shapes {
            let mut took = Vec::with_capacity(h.rounds);
            for round in 0..h.rounds {
                let one = format!("id = \"s{}\"", round % h.signals);
                let r = match shape {
                    "range" => request("entities/dev", one, from, end),
                    "aggregate" => QuerySamplesRequest {
                        aggregation: aggregate(AggregateFunction::Average, Some(60)),
                        ..request("entities/dev", one, from, end)
                    },
                    _ => QuerySamplesRequest {
                        aggregation: aggregate(AggregateFunction::Latest, None),
                        ..request("entities/-", String::new(), from, end)
                    },
                };
                let t = Instant::now();
                let results = query(channel, r).await;
                took.push(t.elapsed().as_secs_f64() * 1e3);
                assert!(results > 0, "{shape} over {label} returned nothing");
            }
            latency(metrics, &format!("history.query.{shape}_{label}"), took);
        }
    }
}

/// A spread of instants inside the set, the same every run.
fn instant(round: usize, seconds: i64) -> i64 {
    EPOCH + 1 + (round as i64 * 2_654_435_761).rem_euclid((seconds - 1).max(1))
}

/// StateAt for one signal and for every signal, at instants spread over the set.
pub async fn states(channel: &Channel, h: &History, metrics: &mut Metrics) {
    for (case, one) in [("one_signal", true), ("all_signals", false)] {
        let mut took = Vec::with_capacity(h.rounds);
        for round in 0..h.rounds {
            let request = ComputeStateRequest {
                parent: "entities/-".into(),
                state_time: at(instant(round, h.seconds)),
                filter: if one {
                    format!("id = \"s{}\"", round % h.signals)
                } else {
                    String::new()
                },
                ..ComputeStateRequest::default()
            };
            let t = Instant::now();
            let known = state(channel, request).await;
            took.push(t.elapsed().as_secs_f64() * 1e3);
            assert_eq!(
                known,
                if one { 1 } else { h.signals },
                "StateAt lost a signal"
            );
        }
        latency(metrics, &format!("history.state_at.{case}"), took);
    }
}

/// One replay of every signal over the first `replay_seconds`, at maximum speed.
pub async fn replays(channel: &Channel, h: &History, metrics: &mut Metrics) {
    let window = h.replay_seconds.min(h.seconds);
    let (samples, seconds) = replay(channel, EPOCH, EPOCH + window).await;
    assert_eq!(
        samples as i64,
        h.signals as i64 * window,
        "replay lost samples"
    );
    let key = "history.replay.samples_per_s";
    metrics.record(
        key,
        "samples/s",
        Better::Higher,
        true,
        samples as f64 / seconds,
    );
}
