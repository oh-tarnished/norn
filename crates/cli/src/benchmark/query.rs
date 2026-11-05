//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The read side of `nornctl benchmark`: repeated `QuerySamples` over what was
//! appended, and the percentiles both sides print.

use std::io::Write;
use std::time::{Duration, Instant};

use norn_protocol::google::r#type::Interval;
use norn_protocol::query::QuerySamplesRequest;
use norn_protocol::query::query_service_client::QueryServiceClient;
use prost_types::Timestamp;
use tonic::transport::Channel;

use super::batch::now_nanos;
use super::plan::Plan;
use crate::CliError;
use crate::resources::call;

/// The nearest-rank `percent`ile of `sorted`, in milliseconds.
pub(super) fn percentile(sorted: &[Duration], percent: usize) -> f64 {
    let rank = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
    sorted.get(rank).map_or(0.0, |d| d.as_secs_f64() * 1e3)
}

/// Runs `plan.queries` reads of everything appended since `first`.
pub(super) async fn query(
    channel: &Channel,
    plan: &Plan,
    first: i128,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let at = |nanos: i128| Timestamp {
        seconds: (nanos / 1_000_000_000) as i64,
        nanos: (nanos % 1_000_000_000) as i32,
    };
    let interval = Interval {
        start_time: Some(at(first)),
        end_time: Some(at(now_nanos() + 1_000_000_000)),
    };
    let mut client = QueryServiceClient::new(channel.clone());
    let (mut samples, mut latencies) = (0u64, Vec::with_capacity(plan.queries));
    let started = Instant::now();
    for _ in 0..plan.queries {
        let request = QuerySamplesRequest {
            parent: format!("entities/{}", plan.stream),
            interval: Some(interval),
            ..QuerySamplesRequest::default()
        };
        let began = Instant::now();
        let call_method = "QueryService.QuerySamples";
        let answer = client
            .query_samples(request)
            .await
            .map_err(call(call_method))?;
        let mut pages = answer.into_inner();
        while let Some(page) = pages.message().await.map_err(call(call_method))? {
            samples += page.samples.len() as u64;
        }
        latencies.push(began.elapsed());
    }
    latencies.sort_unstable();
    let rate = samples as f64 / started.elapsed().as_secs_f64().max(f64::MIN_POSITIVE);
    writeln!(out, "query.calls: {}", plan.queries)?;
    writeln!(out, "query.samples: {samples}")?;
    writeln!(out, "query.samples_per_second: {rate:.0}")?;
    writeln!(out, "query.p50_ms: {:.3}", percentile(&latencies, 50))?;
    writeln!(out, "query.p99_ms: {:.3}", percentile(&latencies, 99))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let d: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert!((percentile(&d, 50) - 50.0).abs() < 1e-9);
        assert!((percentile(&d, 99) - 99.0).abs() < 1e-9);
        assert!((percentile(&d[..1], 99) - 1.0).abs() < 1e-9);
        assert!(percentile(&[], 50).abs() < f64::EPSILON);
    }
}
