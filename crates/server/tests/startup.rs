//! What a start reads: nothing after a clean stop, what could be damaged otherwise.
//! Unix only, as `process/` listens on a Unix socket: see docs/guides/operations.md.
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

use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

use common::{append, batch, double, seed, start, temp};
use norn_protocol::common::ValueType;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_server::DaemonConfig;
use norn_storage::{SegmentReader, StorageOptions};
use process::{connect, spawn};
use query_common::{all, samples};
use tonic::Code;
use upkeep_common::{compact, segments, server, stats, verify};

async fn seeded(tag: &str, rounds: i64) -> PathBuf {
    let d = start(DaemonConfig::default(), temp(tag)).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let mut data = d.stop_keeping().await;
    for round in 0..rounds {
        let d = start(DaemonConfig::default(), data).await;
        let first = round * 10;
        let list = (first..first + 10).map(|n| double("t", n)).collect();
        append(&d.channel, vec![batch(1, list, Durable)]).await;
        data = d.stop_keeping().await;
    }
    data
}

/// Damages the last byte of the first chunk of `segment`.
fn flip(segment: &std::path::Path) {
    let reader = SegmentReader::open(segment, StorageOptions::default()).unwrap();
    let chunk = reader.chunks()[0];
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(segment)
        .unwrap();
    file.seek(SeekFrom::Start(chunk.offset + chunk.frame_len() - 1))
        .unwrap();
    file.write_all(&[0x5a]).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clean_restart_reads_no_chunk_and_totals_the_stream_all_the_same() {
    let d = start(DaemonConfig::default(), seeded("start-clean", 3).await).await;
    let recovery = server(&d.channel).await.recovery.unwrap();
    assert_eq!(recovery.clean_stream_count, 1);
    assert_eq!(recovery.verified_segment_count, 0);
    assert_eq!(recovery.vouched_segment_count, 3);
    let totals = stats(&d.channel).await;
    assert_eq!(totals.sample_count, 30);
    assert_eq!(totals.oldest_sample_time.unwrap().seconds, 1_000);
    assert_eq!(totals.newest_sample_time.unwrap().seconds, 1_029);
    assert!(totals.stored_bytes > 0);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resend_after_a_clean_restart_is_still_a_duplicate() {
    let d = start(DaemonConfig::default(), seeded("start-dup", 1).await).await;
    let list = (0..10).map(|n| double("t", n)).collect();
    let replies = append(&d.channel, vec![batch(1, list, Durable)]).await;
    assert!(
        replies.iter().all(|r| r.duplicate_count == 10),
        "{replies:?}"
    );
    assert_eq!(stats(&d.channel).await.sample_count, 10);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_reads_what_it_could_have_touched_and_trusts_only_what_was_vouched_for() {
    let root = temp("st-c");
    let data = seeded("st-cd", 2).await;
    std::fs::create_dir_all(&root).unwrap();
    let socket = root.join("ipc/n.sock");
    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    let list = (20..30).map(|n| double("t", n)).collect();
    append(&channel, vec![batch(1, list, Durable)]).await;
    drop(channel);
    daemon.kill().unwrap();
    daemon.wait().unwrap();

    let d = start(DaemonConfig::default(), data).await;
    let recovery = server(&d.channel).await.recovery.unwrap();
    assert_eq!(
        recovery.clean_stream_count, 0,
        "a crash is not a clean stop"
    );
    assert_eq!(recovery.vouched_segment_count, 2);
    assert_eq!(recovery.replayed_sample_count, 10);
    assert_eq!(samples(&d.channel, all()).await.len(), 30);
    d.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_compaction_output_after_a_clean_stop_is_read_after_a_crash() {
    let root = temp("st-o");
    std::fs::create_dir_all(&root).unwrap();
    let data = seeded("st-od", 2).await;
    let socket = root.join("ipc/n.sock");
    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    compact(&channel).await;
    drop(channel);
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let d = start(DaemonConfig::default(), data).await;
    let recovery = server(&d.channel).await.recovery.unwrap();
    assert_eq!(recovery.verified_segment_count, 1);
    assert_eq!(recovery.vouched_segment_count, 0);
    d.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_finds_damage_a_clean_start_did_not_read_and_the_next_start_reads_everything() {
    let data = seeded("start-rot", 2).await;
    flip(&segments(&data)[0]);
    let d = start(DaemonConfig::default(), data).await;
    assert_eq!(
        server(&d.channel).await.recovery.unwrap().corruption_count,
        0
    );
    assert_eq!(stats(&d.channel).await.corrupt_sample_count, 0);

    let found = verify(&d.channel).await;
    assert_eq!(found.segment_count, 2);
    assert_eq!(found.damaged_chunk_count, 1);
    assert_eq!(found.damaged_sample_count, 10);
    assert_eq!(found.damaged_chunks.len(), 1);
    let totals = stats(&d.channel).await;
    assert_eq!((totals.sample_count, totals.corrupt_sample_count), (10, 10));
    assert_eq!(
        verify(&d.channel).await.damaged_chunk_count,
        1,
        "verify repeats"
    );
    assert_eq!(
        stats(&d.channel).await.corrupt_sample_count,
        10,
        "counted once"
    );

    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::default(), data).await;
    let recovery = server(&d.channel).await.recovery.unwrap();
    assert_eq!(recovery.clean_stream_count, 0);
    assert_eq!(recovery.verified_segment_count, 2);
    assert_eq!(recovery.corruption_count, 1);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chunk_damaged_while_the_daemon_runs_leaves_the_count_when_a_query_meets_it() {
    let d = start(DaemonConfig::default(), seeded("start-live", 2).await).await;
    assert_eq!(stats(&d.channel).await.sample_count, 20);
    flip(&segments(&d.data)[0]);
    let refused = query_common::pages(&d.channel, all()).await.unwrap_err();
    assert_eq!(refused.code(), Code::DataLoss);
    let totals = stats(&d.channel).await;
    assert_eq!((totals.sample_count, totals.corrupt_sample_count), (10, 10));
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_leaves_a_damaged_segment_alone_and_says_so() {
    let data = seeded("start-skip", 3).await;
    flip(&segments(&data)[1]);
    let d = start(DaemonConfig::default(), data).await;
    let done = compact(&d.channel).await;
    assert_eq!(done.skipped_segment_count, 1);
    let totals = stats(&d.channel).await;
    assert_eq!((totals.sample_count, totals.corrupt_sample_count), (20, 10));
    assert_eq!(
        server(&d.channel)
            .await
            .maintenance
            .unwrap()
            .compaction_failure_count,
        0
    );
    d.stop().await;
}
