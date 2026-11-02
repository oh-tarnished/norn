//! Queries through real gRPC over a Unix socket: docs/performance.md's numbers.
//!
//! `cargo bench -p norn-server --bench query`. Starts the real `norn`, loads a
//! small data set, and measures each query shape closed-loop on one client. `cargo
//! test --all-targets` runs a tiny smoke scale. Data lives under a temp directory
//! removed at the end: about 40 MB at full scale.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "append/driver.rs"]
#[allow(dead_code)]
mod driver;
#[path = "query/load.rs"]
mod load;
#[path = "query/measure.rs"]
mod measure;
#[path = "append/setup.rs"]
#[allow(dead_code)]
mod setup;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use driver::connect;
use measure::{disk, measure, rss_now};
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest, SampleOrder};
use setup::{seed, watch_rss};

const DAY: i64 = 86_400;
const START: i64 = 1_700_000_000;

fn interval(from: i64, seconds: i64) -> Option<Interval> {
    let at = |s| prost_types::Timestamp {
        seconds: s,
        nanos: 0,
    };
    Some(Interval {
        start_time: Some(at(START + from)),
        end_time: Some(at(START + from + seconds)),
    })
}

fn one(signal: usize) -> QuerySamplesRequest {
    QuerySamplesRequest {
        parent: "entities/site".into(),
        filter: format!("id = \"s{signal}\""),
        ..QuerySamplesRequest::default()
    }
}

#[tokio::main]
async fn main() {
    let smoke = !std::env::args().any(|a| a == "--bench");
    let (signals, seconds) = if smoke { (5, 3_600) } else { (200, DAY) };
    let root = std::env::temp_dir().join(format!("norn-qbench-{}", std::process::id()));
    let socket = root.join("ipc/n.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .arg("--listen")
        .arg(format!("unix:{}", socket.display()))
        .args(["--log-level", "warn"])
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let channel = connect(&socket).await;
    let names = seed(&channel, signals).await;
    let began = Instant::now();
    load::load(&channel, &names, START, seconds).await;
    println!(
        "loaded {} samples in {:.1?}",
        signals as i64 * seconds,
        began.elapsed()
    );
    if !smoke {
        // Past `max_chunk_age`, so these read chunks; a second pass reads the tail.
        tokio::time::sleep(Duration::from_secs(62)).await;
    }
    let pid = daemon.id();
    println!("daemon RSS before queries {} MiB", rss_now(pid) / 1024);
    let (rss, stop) = watch_rss(pid);
    let mut client = QueryServiceClient::new(channel.clone());
    println!("| Query | Rounds | Results | p50 | p99 | Samples/s | RSS after |");
    println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    let hours = (seconds / 3_600 - 1).max(1) as usize;
    let rounds = |n| if smoke { 3 } else { n };
    measure(
        "one signal, one hour, raw",
        rounds(500),
        &mut client,
        pid,
        |r| {
            let hour = (r * 7 % hours) as i64;
            QuerySamplesRequest {
                interval: interval(hour * 3_600, 3_600),
                ..one(r % signals)
            }
        },
    )
    .await;
    measure(
        "one signal, whole set, AVERAGE per minute",
        rounds(100),
        &mut client,
        pid,
        |r| QuerySamplesRequest {
            interval: interval(0, seconds),
            aggregation: Some(Aggregation {
                aggregate_function: AggregateFunction::Average as i32,
                window: Some(prost_types::Duration {
                    seconds: 60,
                    nanos: 0,
                }),
            }),
            ..one(r % signals)
        },
    )
    .await;
    let everything = |r: QuerySamplesRequest| QuerySamplesRequest {
        parent: "entities/-".into(),
        ..r
    };
    measure("all signals, LATEST", rounds(100), &mut client, pid, |_| {
        everything(QuerySamplesRequest {
            aggregation: Some(Aggregation {
                aggregate_function: AggregateFunction::Latest as i32,
                window: None,
            }),
            ..QuerySamplesRequest::default()
        })
    })
    .await;
    measure(
        "all signals, one hour, by time",
        rounds(5),
        &mut client,
        pid,
        |_| {
            everything(QuerySamplesRequest {
                interval: interval(0, 3_600.min(seconds)),
                order: SampleOrder::ByTime as i32,
                ..QuerySamplesRequest::default()
            })
        },
    )
    .await;
    stop.cancel();
    println!("peak daemon RSS {} MiB", rss.await.unwrap() / 1024);
    let bytes = disk(&root.join("data"));
    println!("data on disk {} MiB", bytes >> 20);
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
