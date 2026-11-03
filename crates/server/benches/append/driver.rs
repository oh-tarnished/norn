//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Driving a daemon: seeding, open-loop sending, latency, and RSS.

use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest, BackpressureOutcome};
use tokio::sync::mpsc;
use tonic::codegen::tokio_stream::Stream as TokioStream;
use tonic::transport::Channel;

#[derive(Debug, Clone, Copy)]
pub struct Load {
    pub name: &'static str,
    pub rate: u64,
    pub batch: u64,
    pub secs: u64,
    pub level: AcknowledgementLevel,
}

pub struct Report {
    pub rate: f64,
    pub p50: Duration,
    pub p99: Duration,
    pub throttled: u64,
    pub rejected: u64,
}

pub async fn connect(socket: &Path) -> Channel {
    let endpoint = Endpoint::Unix(socket.to_path_buf());
    loop {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            let get = GetServerRequest {
                name: "server".into(),
            };
            if AdminServiceClient::new(channel.clone())
                .get_server(get)
                .await
                .is_ok()
            {
                return channel;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub struct Requests(pub mpsc::Receiver<AppendSamplesRequest>);

impl TokioStream for Requests {
    type Item = AppendSamplesRequest;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// A realistic value: a slow drift to one decimal, per signal.
fn sample(names: &[String], n: i64) -> Sample {
    let count = names.len() as i64;
    let signal = names[(n % count) as usize].clone();
    let tick = n / count;
    Sample {
        signal,
        observe_time: Some(prost_types::Timestamp {
            seconds: 1_700_000_000 + tick,
            nanos: 0,
        }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(20.0 + ((tick + n) % 100) as f64 / 10.0)),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence: n,
    }
}

/// Offers `load` at its rate on one call, starting at sample number `base`.
pub async fn drive(channel: &Channel, names: &Arc<Vec<String>>, load: Load, base: i64) -> Report {
    let (tx, rx) = mpsc::channel(64);
    let sent: Arc<Mutex<HashMap<i64, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let (hold_tx, mut hold_rx) = mpsc::unbounded_channel::<Duration>();
    let (names2, times) = (Arc::clone(names), Arc::clone(&sent));
    let sender = tokio::spawn(async move {
        let batches = load.rate * load.secs / load.batch;
        let every = Duration::from_secs_f64(load.batch as f64 / load.rate as f64);
        let mut tick = tokio::time::interval(every);
        for id in 0..batches as i64 {
            tick.tick().await;
            while let Ok(delay) = hold_rx.try_recv() {
                tokio::time::sleep(delay).await;
            }
            let start = base + id * load.batch as i64;
            let samples = (start..start + load.batch as i64)
                .map(|n| sample(&names2, n))
                .collect();
            times.lock().unwrap().insert(id + 1, Instant::now());
            let request = AppendSamplesRequest {
                stream: "streams/plant".into(),
                batch_id: id + 1,
                samples,
                ack_level: load.level as i32,
            };
            if tx.send(request).await.is_err() {
                return;
            }
        }
    });
    let begun = Instant::now();
    let mut client = TelemetryServiceClient::new(channel.clone());
    let mut replies = client
        .append_samples(Requests(rx))
        .await
        .unwrap()
        .into_inner();
    let (mut latencies, mut throttled, mut rejected, mut done) = (Vec::new(), 0, 0, 0u64);
    while let Some(r) = replies.message().await.unwrap() {
        let a = r.admission.clone().unwrap_or_default();
        if a.outcome == BackpressureOutcome::Reject as i32 {
            rejected += 1;
            continue;
        }
        if a.outcome == BackpressureOutcome::Throttle as i32 && r.ack_level != load.level as i32 {
            throttled += 1;
            let delay = a.retry_delay.unwrap_or_default();
            let _ = hold_tx.send(Duration::try_from(delay).unwrap_or_default());
        }
        if r.ack_level == load.level as i32 {
            done += load.batch;
            if let Some(at) = sent.lock().unwrap().remove(&r.batch_id) {
                latencies.push(at.elapsed());
            }
        }
    }
    sender.await.unwrap();
    let elapsed = begun.elapsed().as_secs_f64();
    latencies.sort_unstable();
    let at = |q: f64| latencies[((latencies.len() - 1) as f64 * q) as usize];
    Report {
        rate: done as f64 / elapsed,
        p50: at(0.5),
        p99: at(0.99),
        throttled,
        rejected,
    }
}
