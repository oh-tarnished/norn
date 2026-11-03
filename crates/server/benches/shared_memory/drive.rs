//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One open-loop run through `norn_ipc::AppendCall`, on whichever plane the
//! options ask for: the same client code for the socket and for shared memory,
//! so the planes are all that differs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use norn_ipc::{AppendCall, AppendOptions, DataPlaneKind};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value};
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest, BackpressureOutcome};
use tonic::transport::Channel;

/// An offered load: `rate` samples a second in batches of `batch`, for `secs`.
#[derive(Debug, Clone, Copy)]
pub struct Load {
    pub rate: u64,
    pub batch: u64,
    pub secs: u64,
}

/// What a run achieved.
pub struct Report {
    pub plane: DataPlaneKind,
    pub rate: f64,
    pub p50: Duration,
    pub p99: Duration,
    pub rejected: u64,
}

fn sample(names: &[String], n: i64) -> Sample {
    let signal = names[n as usize % names.len()].clone();
    Sample {
        signal,
        observe_time: Some(prost_types::Timestamp {
            seconds: 1_700_000_000 + n / 1_000,
            nanos: i32::try_from(n % 1_000).unwrap() * 1_000_000,
        }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(n as f64 * 0.5)),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence: n,
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let at = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[at]
}

/// Sends `load` from sequence `base` and measures each batch's first answer
/// (`BUFFERED`): the time from handing it over to hearing it was admitted.
pub async fn run(
    channel: &Channel,
    options: &AppendOptions,
    names: &Arc<Vec<String>>,
    load: Load,
    base: i64,
) -> Report {
    let call = AppendCall::open(channel.clone(), options).await.unwrap();
    let plane = call.plane();
    let (sender, mut replies) = call.into_parts();
    let sent_at = Arc::new(Mutex::new(HashMap::<i64, Instant>::new()));
    let batches = load.rate * load.secs / load.batch;
    let every = Duration::from_secs_f64(load.batch as f64 / load.rate as f64);
    let (names, stamps) = (Arc::clone(names), Arc::clone(&sent_at));
    let writer = tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
        for id in 1..=i64::try_from(batches).unwrap() {
            tick.tick().await;
            let from = base + (id - 1) * load.batch as i64;
            let samples = (from..from + load.batch as i64)
                .map(|n| sample(&names, n))
                .collect();
            let request = AppendSamplesRequest {
                stream: "streams/plant".into(),
                batch_id: id,
                samples,
                ack_level: AcknowledgementLevel::Buffered as i32,
            };
            stamps.lock().unwrap().insert(id, Instant::now());
            sender.send(request).await.unwrap();
        }
    });
    let started = Instant::now();
    let (mut latencies, mut admitted, mut rejected) = (Vec::new(), 0u64, 0u64);
    while let Some(reply) = replies.message().await.unwrap() {
        let Some(at) = sent_at.lock().unwrap().remove(&reply.batch_id) else {
            continue;
        };
        latencies.push(at.elapsed());
        let outcome = reply.admission.map(|a| a.outcome);
        if outcome == Some(BackpressureOutcome::Reject as i32) {
            rejected += 1;
        } else {
            admitted += load.batch;
        }
    }
    writer.await.unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    latencies.sort();
    Report {
        plane,
        rate: admitted as f64 / elapsed,
        p50: percentile(&latencies, 0.50),
        p99: percentile(&latencies, 0.99),
        rejected,
    }
}
