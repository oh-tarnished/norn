//! kill -9 at every phase of a compaction, in a real second process.
//!
//! The child is this test binary run again: `park_at_a_phase` is a real daemon that
//! stops dead at the phase named in its environment, so `norn` itself carries no
//! fault hook. Everything durable must be readable once, whichever phase it died in.
//! Unix only: the child listens on a Unix socket (docs/guides/operations.md,
//! "Platforms").
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod process;
mod query_common;
mod upkeep_common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{append, batch, double, seed, start, temp};
use norn_ipc::Endpoint;
use norn_protocol::common::ValueType;
use norn_protocol::stream::CompactStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_runtime::maintenance::CompactionPhase;
use norn_server::{Daemon, DaemonConfig};
use process::connect;
use query_common::{all, samples};
use tokio_util::sync::CancellationToken;
use upkeep_common::{PLANT, compact, segments, stats, verify};

const PHASE: &str = "NORN_TEST_PHASE";
const DATA: &str = "NORN_TEST_DATA";
const SOCKET: &str = "NORN_TEST_SOCKET";

fn named(phase: &str) -> Option<CompactionPhase> {
    Some(match phase {
        "merging" => CompactionPhase::Merging,
        "written" => CompactionPhase::OutputWritten,
        "committed" => CompactionPhase::Committed,
        "input" => CompactionPhase::InputRemoved,
        "inputs" => CompactionPhase::InputsRemoved,
        _ => return None,
    })
}

/// Not a test: the child, which only ever runs with its environment set.
#[test]
#[ignore = "the child process of the kill tests"]
fn park_at_a_phase() {
    let target = named(&std::env::var(PHASE).unwrap()).unwrap();
    let data = PathBuf::from(std::env::var(DATA).unwrap());
    let socket = PathBuf::from(std::env::var(SOCKET).unwrap());
    let reached = data.with_extension("reached");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async move {
        let config = DaemonConfig::default()
            .listen(Endpoint::Unix(socket))
            .data_dir(data);
        let daemon = Daemon::bind(config).await.unwrap();
        daemon.compaction_probe(Box::new(move |phase| {
            if phase == target {
                std::fs::write(&reached, b"").unwrap();
                std::thread::park();
            }
        }));
        daemon.run(CancellationToken::new()).await.unwrap();
    });
}

async fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "the child never reached its phase"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn killed_during(phase: &str) {
    let root = temp(&format!("k-{phase}"));
    std::fs::create_dir_all(&root).unwrap();
    let (data, socket) = (root.join("data"), root.join("ipc/n.sock"));
    // Three sealed segments, and every sample durable.
    let d = start(DaemonConfig::default(), data.clone()).await;
    seed(
        &d.channel,
        &[("t", ValueType::Double), ("u", ValueType::Double)],
    )
    .await;
    let mut data = d.stop_keeping().await;
    for round in 0..3 {
        let d = start(DaemonConfig::default(), data).await;
        let first = round * 100;
        let list = (first..first + 100)
            .map(|n| double(if n % 2 == 0 { "t" } else { "u" }, n))
            .collect();
        append(&d.channel, vec![batch(1, list, Durable)]).await;
        data = d.stop_keeping().await;
    }
    assert_eq!(segments(&data).len(), 3);

    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(exe)
        .args(["--ignored", "--exact", "park_at_a_phase", "--nocapture"])
        .env(PHASE, phase)
        .env(DATA, &data)
        .env(SOCKET, &socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let channel = connect(&socket).await;
    let request = CompactStreamRequest {
        name: PLANT.into(),
        interval: None,
    };
    let mut streams = StreamServiceClient::new(channel);
    streams.compact_stream(request).await.unwrap();
    wait_for(&data.with_extension("reached")).await;
    child.kill().unwrap();
    child.wait().unwrap();

    let d = start(DaemonConfig::default(), data).await;
    let mut seqs: Vec<i64> = samples(&d.channel, all())
        .await
        .iter()
        .map(|s| s.sequence)
        .collect();
    seqs.sort_unstable();
    assert_eq!(seqs, (0..300).collect::<Vec<_>>(), "after dying at {phase}");
    let totals = stats(&d.channel).await;
    assert_eq!(totals.sample_count, 300, "after dying at {phase}");
    assert_eq!(totals.corrupt_sample_count, 0);
    assert_eq!(verify(&d.channel).await.damaged_chunk_count, 0);
    compact(&d.channel).await;
    let on_disk = segments(&d.data);
    assert_eq!(on_disk.len(), 1, "after dying at {phase}: {on_disk:?}");
    let tmp = std::fs::read_dir(on_disk[0].parent().unwrap()).unwrap();
    assert!(
        !tmp.flatten()
            .any(|e| e.path().to_string_lossy().ends_with(".tmp"))
    );
    let again = samples(&d.channel, all()).await;
    assert_eq!(again.len(), 300, "after compacting again from {phase}");
    d.stop_keeping().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn dying_mid_merge_leaves_the_inputs_serving() {
    killed_during("merging").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dying_with_the_output_written_and_uncommitted_loses_nothing() {
    killed_during("written").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dying_right_after_the_commit_doubles_nothing() {
    killed_during("committed").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dying_with_some_inputs_deleted_doubles_nothing() {
    killed_during("input").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dying_before_the_index_hears_of_it_loses_nothing() {
    killed_during("inputs").await;
}
