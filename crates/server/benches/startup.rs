//! Startup cost: spawning the real `norn` until it answers, over a real database.
//!
//! `cargo bench -p norn-server --bench startup`. Loads 16,000 signals for two
//! hours, then times a start three ways: with the clean marker, with the marker
//! removed (every chunk read and checked, which is what every start once did), and
//! after kill -9 with some new data. `cargo test --all-targets` runs a tiny scale.
//! The page cache stays warm, so the unmarked figure is CPU, not a cold disk: a
//! cold start reads every byte and is slower by the disk. Data lives under a temp
//! directory removed at the end.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "append/driver.rs"]
#[allow(dead_code)]
mod driver;
#[path = "query/load.rs"]
mod load;
#[path = "append/setup.rs"]
#[allow(dead_code)]
mod setup;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use driver::connect;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{GetServerRequest, RecoveryReport};
use tonic::transport::Channel;

const START: i64 = 1_700_000_000;
const RUNS: usize = 3;

fn spawn(root: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .arg("--listen")
        .arg(format!("unix:{}", root.join("ipc/n.sock").display()))
        .args(["--log-level", "error"])
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

fn terminate(mut daemon: Child) {
    let id = daemon.id().to_string();
    Command::new("kill").args(["-TERM", &id]).status().unwrap();
    assert!(daemon.wait().unwrap().success());
}

async fn recovery(channel: &Channel) -> RecoveryReport {
    let name = "server".to_owned();
    let got = AdminServiceClient::new(channel.clone())
        .get_server(GetServerRequest { name })
        .await;
    got.unwrap().into_inner().recovery.unwrap()
}

async fn timed(root: &Path) -> (Child, Channel, Duration) {
    let began = Instant::now();
    let daemon = spawn(root);
    let channel = connect(&root.join("ipc/n.sock")).await;
    (daemon, channel, began.elapsed())
}

fn report(label: &str, took: Duration, r: &RecoveryReport) {
    println!(
        "{label}: {took:.2?} clean_streams={} verified_segments={} vouched_segments={} replayed={}",
        r.clean_stream_count,
        r.verified_segment_count,
        r.vouched_segment_count,
        r.replayed_sample_count
    );
}

fn remove_markers(root: &Path) {
    for dir in std::fs::read_dir(root.join("data/streams")).unwrap() {
        let _ = std::fs::remove_file(dir.unwrap().path().join("CLEAN"));
    }
}

#[tokio::main]
async fn main() {
    let smoke = !std::env::args().any(|a| a == "--bench");
    let (signals, seconds) = if smoke { (5, 60) } else { (16_000, 7_200) };
    let root = std::env::temp_dir().join(format!("norn-sbench-{}", std::process::id()));
    let daemon = spawn(&root);
    let channel = connect(&root.join("ipc/n.sock")).await;
    let names = setup::seed(&channel, signals).await;
    let began = Instant::now();
    load::load(&channel, &names, START, seconds).await;
    println!(
        "loaded {} samples in {:.1?}",
        signals as i64 * seconds,
        began.elapsed()
    );
    terminate(daemon);
    for _ in 0..RUNS {
        let (daemon, channel, took) = timed(&root).await;
        report("marked", took, &recovery(&channel).await);
        terminate(daemon);
    }
    for _ in 0..RUNS {
        remove_markers(&root);
        let (daemon, channel, took) = timed(&root).await;
        report("unmarked", took, &recovery(&channel).await);
        terminate(daemon);
    }
    let (mut daemon, channel, _) = timed(&root).await;
    load::load(&channel, &Arc::clone(&names), START + seconds, 60).await;
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let (daemon, channel, took) = timed(&root).await;
    report("after kill -9", took, &recovery(&channel).await);
    terminate(daemon);
    let _ = std::fs::remove_dir_all(&root);
}
