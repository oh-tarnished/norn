//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Round-trip latency of a small unary gRPC call (`grpc.health.v1.Check`),
//! Unix socket vs loopback TCP. Results are recorded in docs/reference/ipc.md.
//!
//! ```sh
//! cargo run --release -p norn-ipc --example latency -- 20000
//! ```
//!
//! Calls are sequential on one channel, after a warm-up, so the figure is
//! per-call latency (client encode → transport → server → back), not
//! throughput.

use std::time::{Duration, Instant};

use norn_ipc::{Endpoint, Server, ServerConfig, connect};
use tokio_util::sync::CancellationToken;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_client::HealthClient;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

const WARMUP: usize = 1_000;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let calls: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(10_000);
    let dir = std::env::temp_dir().join(format!("norn-lat-{}", std::process::id()));
    let mut endpoints = vec![("tcp", "tcp:127.0.0.1:0".parse::<Endpoint>()?)];
    if cfg!(unix) {
        endpoints.insert(0, ("unix", Endpoint::Unix(dir.join("n.sock"))));
    }
    println!("{calls} sequential unary calls after {WARMUP} warm-up");
    println!(
        "{:<6} {:>9} {:>9} {:>9} {:>9}",
        "", "p50 µs", "p99 µs", "mean µs", "max µs"
    );
    for (name, endpoint) in endpoints {
        let mut samples = measure(&endpoint, calls).await?;
        samples.sort_unstable();
        let pick = |q: f64| percentile(&samples, q);
        let total: Duration = samples.iter().sum();
        let mean = total.as_secs_f64() * 1e6 / samples.len().max(1) as f64;
        println!(
            "{name:<6} {:>9.1} {:>9.1} {mean:>9.1} {:>9.1}",
            pick(0.50),
            pick(0.99),
            pick(1.0)
        );
    }
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

async fn measure(endpoint: &Endpoint, calls: usize) -> Result<Vec<Duration>, BoxError> {
    let server = Server::bind(endpoint, ServerConfig::default()).await?;
    let bound = server.local_endpoint().clone();
    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let router = tonic::transport::Server::builder().add_service(health);
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(server.serve(router, shutdown.clone()));

    let mut client = HealthClient::new(connect(&bound).await?);
    let mut samples = Vec::with_capacity(calls);
    for i in 0..WARMUP + calls {
        let started = Instant::now();
        client.check(HealthCheckRequest::default()).await?;
        if i >= WARMUP {
            samples.push(started.elapsed());
        }
    }
    drop(client);
    shutdown.cancel();
    task.await??;
    Ok(samples)
}

/// The `q` quantile of sorted `samples`, in microseconds.
fn percentile(samples: &[Duration], q: f64) -> f64 {
    let last = samples.len().saturating_sub(1);
    let idx = ((last as f64) * q).round() as usize;
    samples
        .get(idx.min(last))
        .map_or(0.0, |d| d.as_secs_f64() * 1e6)
}
