//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One open-loop ingest run through `norn_ipc::AppendCall`: the same client code
//! on the socket and on a shared-memory ring, so the plane is all that differs.
//!
//! Batches leave on a fixed schedule whatever the daemon answers (open loop),
//! except that a `THROTTLE` delays the next one by its `retry_delay`, as a client
//! must. A batch's latency runs from handing it over to the first response at or
//! above the requested level: each level implies those below it, and the daemon
//! answers RECEIVED with the level the batch has actually reached. A client that cannot build batches fast enough falls behind
//! the schedule, and the achieved rate shows it: the client shares the machine.
//! A refused batch is counted and not sent again, and does not pause the schedule.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use norn_ipc::{AppendCall, AppendOptions, DataPlaneKind};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value};
use norn_protocol::telemetry::{AppendSamplesRequest, BackpressureOutcome};
use tokio::sync::mpsc;
use tonic::transport::Channel;

use crate::profile::IngestRow;
use crate::seed::STREAM;

/// Observe time of the first sample; history, so no clock check objects.
pub const EPOCH: i64 = 1_700_000_000;

/// What one run achieved.
pub struct Outcome {
    pub plane: DataPlaneKind,
    pub achieved: f64,
    /// Every acknowledged batch's latency, in milliseconds, ascending.
    pub acks_ms: Vec<f64>,
    pub throttled: u64,
    /// Batches refused at admission, or that failed before reaching their level.
    pub rejected: u64,
}

/// Sample `n` of a row: signal `n mod S`, one second of observe time per
/// round of signals, so every signal is 1 Hz in observe time whatever the
/// offered rate. A drift to 0.1 with 0.01 of deterministic noise.
fn sample(names: &[String], n: i64) -> Sample {
    let count = names.len().max(1) as i64;
    let signal = names.get((n % count) as usize).cloned().unwrap_or_default();
    let tick = n / count;
    let mix = (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let noise = (mix >> 54) as f64 / 100.0;
    Sample {
        signal,
        observe_time: Some(prost_types::Timestamp {
            seconds: EPOCH + tick,
            nanos: 0,
        }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(
                20.0 + (tick % 1_000) as f64 / 10.0 + noise,
            )),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence: n,
    }
}

/// Offers `row` to the daemon behind `channel`, with `options` choosing the plane.
pub async fn drive(
    channel: &Channel,
    options: &AppendOptions,
    names: &Arc<Vec<String>>,
    row: &IngestRow,
) -> Outcome {
    let call = AppendCall::open(channel.clone(), options).await.unwrap();
    let plane = call.plane();
    let (sender, mut replies) = call.into_parts();
    let sent: Arc<Mutex<HashMap<i64, Instant>>> = Arc::default();
    let (hold_tx, mut hold_rx) = mpsc::channel::<Duration>(1_024);
    let (names, stamps, load) = (Arc::clone(names), Arc::clone(&sent), row.clone());
    let writer = tokio::spawn(async move {
        let batches = (load.rate * load.secs / load.batch).max(1) as i64;
        let every = Duration::from_secs_f64(load.batch as f64 / load.rate as f64);
        // The first batch leaves one period in, so N batches take N periods and an
        // offered rate that is met reads as that rate, not above it.
        let start = tokio::time::Instant::now() + every;
        let mut tick = tokio::time::interval_at(start, every);
        let size = load.batch as i64;
        for id in 1..=batches {
            tick.tick().await;
            while let Ok(delay) = hold_rx.try_recv() {
                tokio::time::sleep(delay).await;
            }
            let from = (id - 1) * size;
            let request = AppendSamplesRequest {
                stream: STREAM.into(),
                batch_id: id,
                samples: (from..from + size).map(|n| sample(&names, n)).collect(),
                ack_level: load.level as i32,
            };
            stamps.lock().unwrap().insert(id, Instant::now());
            if sender.send(request).await.is_err() {
                return;
            }
        }
    });
    let began = Instant::now();
    let level = row.level as i32;
    let (mut acks_ms, mut throttled, mut rejected, mut done) = (Vec::new(), 0, 0, 0u64);
    let mut seen = HashSet::new();
    let limit = Duration::from_secs(row.secs * 20 + 120);
    let reading = async {
        while let Some(r) = replies.message().await.unwrap() {
            let a = r.admission.clone().unwrap_or_default();
            // A completion repeats the admission of the response before it.
            let first = seen.insert(r.batch_id);
            if a.outcome == BackpressureOutcome::Reject as i32 || r.error.is_some() {
                rejected += 1;
                sent.lock().unwrap().remove(&r.batch_id);
                continue;
            }
            if first && a.outcome == BackpressureOutcome::Throttle as i32 {
                throttled += 1;
                let delay = Duration::try_from(a.retry_delay.unwrap_or_default());
                let _ = hold_tx.try_send(delay.unwrap_or_default());
            }
            if r.ack_level >= level
                && let Some(at) = sent.lock().unwrap().remove(&r.batch_id)
            {
                done += row.batch;
                acks_ms.push(at.elapsed().as_secs_f64() * 1e3);
            }
        }
    };
    tokio::time::timeout(limit, reading)
        .await
        .expect("the row stalled");
    writer.await.unwrap();
    acks_ms.sort_by(f64::total_cmp);
    Outcome {
        plane,
        achieved: done as f64 / began.elapsed().as_secs_f64(),
        acks_ms,
        throttled,
        rejected,
    }
}
