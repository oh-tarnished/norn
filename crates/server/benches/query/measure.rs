//! Timing one query shape, and what the daemon holds.

use std::process::Command;
use std::time::Instant;

use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{QuerySamplesRequest, QuerySamplesResponse};
use tonic::transport::Channel;

async fn run(client: &mut QueryServiceClient<Channel>, r: QuerySamplesRequest) -> (u64, u64) {
    let mut stream = client.query_samples(r).await.unwrap().into_inner();
    let (mut results, mut reduced) = (0, 0);
    while let Some(QuerySamplesResponse {
        samples,
        aggregates,
        ..
    }) = stream.message().await.unwrap()
    {
        results += (samples.len() + aggregates.len()) as u64;
        reduced += samples.len() as u64;
        reduced += aggregates
            .iter()
            .map(|a| a.sample_count as u64)
            .sum::<u64>();
    }
    (results, reduced)
}

pub async fn measure(
    name: &str,
    rounds: usize,
    client: &mut QueryServiceClient<Channel>,
    pid: u32,
    make: impl Fn(usize) -> QuerySamplesRequest,
) {
    let (mut took, mut results, mut reduced) = (Vec::new(), 0, 0);
    let began = Instant::now();
    for round in 0..rounds {
        let t = Instant::now();
        let (r, s) = run(client, make(round)).await;
        took.push(t.elapsed());
        results += r;
        reduced += s;
    }
    let total = began.elapsed().as_secs_f64();
    took.sort_unstable();
    let at = |q: usize| took[(took.len() - 1) * q / 100];
    println!(
        "| {name} | {rounds} | {} | {:.2?} | {:.2?} | {:.0} | {} MiB |",
        results / rounds as u64,
        at(50),
        at(99),
        reduced as f64 / total,
        rss_now(pid) / 1024,
    );
}

/// The daemon's resident size now, in KiB.
pub fn rss_now(pid: u32) -> u64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

pub fn disk(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => disk(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}
