//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Timing `ComputeState`, and a stand-in for what checkpoints would cost.

use std::future::Future;
use std::time::{Duration, Instant};

use norn_protocol::google::r#type::Interval;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest};
use norn_protocol::state::ComputeStateRequest;
use norn_protocol::state::state_service_client::StateServiceClient;
use tonic::transport::Channel;

pub fn at(seconds: i64) -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp { seconds, nanos: 0 })
}

pub fn row(name: &str, took: &mut [Duration]) {
    took.sort_unstable();
    let at = |q: usize| took[(took.len() - 1) * q / 100];
    let rounds = took.len();
    println!("| {name} | {rounds} | {:.2?} | {:.2?} |", at(50), at(99));
}

pub async fn timed<F: Future<Output = ()>>(
    rounds: usize,
    mut once: impl FnMut(usize) -> F,
) -> Vec<Duration> {
    let mut took = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let t = Instant::now();
        once(round).await;
        took.push(t.elapsed());
    }
    took
}

/// Every page of one state; returns how many signals had a sample.
pub async fn state(channel: &Channel, mut r: ComputeStateRequest) -> usize {
    let mut client = StateServiceClient::new(channel.clone());
    r.page_size = 10_000;
    let mut known = 0;
    loop {
        let page = client.compute_state(r.clone()).await.unwrap().into_inner();
        known += page
            .signal_states
            .iter()
            .filter(|s| s.sample.is_some())
            .count();
        if page.next_page_token.is_empty() {
            return known;
        }
        r.page_token = page.next_page_token;
    }
}

/// What a checkpoint at `from` would still have to do to reach `to`: read every
/// sample between, keeping each signal's last. Loading the checkpoint itself is
/// taken as free, and no sample crosses the wire, so this is a lower bound.
pub async fn changes_since(channel: &Channel, from: i64, to: i64) -> u64 {
    let request = QuerySamplesRequest {
        parent: "entities/-".into(),
        interval: Some(Interval {
            start_time: at(from),
            end_time: at(to + 1),
        }),
        aggregation: Some(Aggregation {
            aggregate_function: AggregateFunction::Last as i32,
            window: None,
        }),
        ..QuerySamplesRequest::default()
    };
    let mut client = QueryServiceClient::new(channel.clone());
    let mut stream = client.query_samples(request).await.unwrap().into_inner();
    let mut samples = 0;
    while let Some(page) = stream.message().await.unwrap() {
        samples += page
            .aggregates
            .iter()
            .map(|a| a.sample_count as u64)
            .sum::<u64>();
    }
    samples
}
