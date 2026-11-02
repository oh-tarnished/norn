//! Compaction in a running daemon, with ingest and readers going.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod query_common;
mod upkeep_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use common::{append, batch, double, seed, start, temp};
use norn_core::{Config, MaintenanceConfig};
use norn_protocol::common::ValueType;
use norn_protocol::state::ComputeStateRequest;
use norn_protocol::state::state_service_client::StateServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_server::DaemonConfig;
use query_common::{all, samples};
use tonic::transport::Channel;
use upkeep_common::{compact, rolling, segments, server, stats, verify};

const SIGNALS: [&str; 4] = ["a", "b", "c", "d"];

fn kinds() -> [(&'static str, ValueType); 4] {
    SIGNALS.map(|s| (s, ValueType::Double))
}

/// Appends batch `id` of 40 samples, sequences `40 * id..`, each acknowledged.
async fn append_one(channel: &Channel, id: i64) {
    let from = 40 * id;
    let list = (from..from + 40)
        .map(|n| double(SIGNALS[(n % 4) as usize], n))
        .collect();
    append(channel, vec![batch(id + 1, list, Persisted)]).await;
}

/// No sample twice, and every one of the first `acked` sequences present. A batch
/// still being committed may show for some signals and not yet for others: a read
/// scans signal by signal, which is the read path's rule and not compaction's.
async fn check_whole(channel: &Channel, acked: i64) -> i64 {
    let got = samples(channel, all()).await;
    let mut seqs: Vec<i64> = got.iter().map(|s| s.sequence).collect();
    seqs.sort_unstable();
    let repeat = seqs.windows(2).find(|w| w[0] == w[1]);
    assert!(repeat.is_none(), "a repeat: {repeat:?}");
    let prefix = seqs.iter().take_while(|n| **n < acked).count() as i64;
    assert_eq!(prefix, acked, "lost an acknowledged sample below {acked}");
    let present: std::collections::HashSet<i64> = seqs.iter().copied().collect();
    let gap = (0..acked).find(|n| !present.contains(n));
    assert!(gap.is_none(), "acknowledged sequence {gap:?} is missing");
    seqs.len() as i64
}

#[tokio::test(flavor = "multi_thread")]
async fn queries_and_state_lose_and_double_nothing_while_ingest_and_compaction_run() {
    let d = start(DaemonConfig::new(rolling()), temp("compact-live")).await;
    seed(&d.channel, &kinds()).await;
    for id in 0..400 {
        append_one(&d.channel, id).await;
    }
    let next = Arc::new(AtomicI64::new(400));
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (channel, next, stop) = (d.channel.clone(), Arc::clone(&next), Arc::clone(&stop));
        tokio::spawn(async move {
            while !stop.load(Ordering::Acquire) {
                let id = next.fetch_add(1, Ordering::AcqRel);
                append_one(&channel, id).await;
            }
        })
    };
    let reader = {
        let (channel, next, stop) = (d.channel.clone(), Arc::clone(&next), Arc::clone(&stop));
        tokio::spawn(async move {
            let mut reads = 0;
            while !stop.load(Ordering::Acquire) {
                // Batches below the one the writer last took are acknowledged.
                let acknowledged = (next.load(Ordering::Acquire) - 1).max(0) * 40;
                check_whole(&channel, acknowledged).await;
                let request = ComputeStateRequest {
                    parent: "entities/ahu".into(),
                    state_time: Some(prost_types::Timestamp {
                        seconds: 100_000,
                        nanos: 0,
                    }),
                    ..ComputeStateRequest::default()
                };
                let mut state = StateServiceClient::new(channel.clone());
                let got = state.compute_state(request).await.unwrap().into_inner();
                let known = got.signal_states.iter().filter(|s| s.sample.is_some());
                assert_eq!(known.count(), SIGNALS.len());
                reads += 1;
            }
            reads
        })
    };
    let before = segments(&d.data).len();
    let mut merged = 0;
    for _ in 0..6 {
        let done = compact(&d.channel).await;
        merged += done.original_chunk_count - done.compacted_chunk_count;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stop.store(true, Ordering::Release);
    writer.await.unwrap();
    let reads = reader.await.unwrap();
    assert!(before > 1, "ingest never rolled a segment: {before}");
    assert!(merged > 0, "compaction merged nothing");
    assert!(reads > 0, "no read ran during compaction");
    let total = next.load(Ordering::Acquire) * 40;
    assert_eq!(check_whole(&d.channel, total).await, total);
    assert_eq!(stats(&d.channel).await.sample_count, total);
    let found = verify(&d.channel).await;
    assert_eq!(found.damaged_chunk_count, 0);
    let report = server(&d.channel).await.maintenance.unwrap();
    assert!(report.compaction_count > 0);
    assert_eq!(report.compaction_failure_count, 0);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_compacts_by_itself_on_its_schedule() {
    let mut schedule = MaintenanceConfig::default();
    schedule.compaction_interval = Duration::from_millis(30);
    let engine = Config::builder().maintenance(schedule).build().unwrap();
    let d = start(DaemonConfig::new(engine), temp("compact-sched")).await;
    seed(&d.channel, &kinds()).await;
    append_one(&d.channel, 0).await;
    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::new(engine), data).await;
    append_one(&d.channel, 1).await;
    let data = d.stop_keeping().await;
    assert_eq!(segments(&data).len(), 2);
    let d = start(DaemonConfig::new(engine), data).await;
    for _ in 0..500 {
        if server(&d.channel)
            .await
            .maintenance
            .unwrap()
            .compaction_count
            > 0
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(segments(&d.data).len(), 1);
    assert_eq!(check_whole(&d.channel, 80).await, 80);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_told_to_stay_out_of_the_background_compacts_only_when_asked() {
    let mut schedule = MaintenanceConfig::default();
    schedule.background = false;
    schedule.compaction_interval = Duration::from_millis(5);
    let engine = Config::builder().maintenance(schedule).build().unwrap();
    let d = start(DaemonConfig::new(engine), temp("compact-off")).await;
    seed(&d.channel, &kinds()).await;
    append_one(&d.channel, 0).await;
    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::new(engine), data).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        server(&d.channel)
            .await
            .maintenance
            .unwrap()
            .compaction_count,
        0
    );
    compact(&d.channel).await;
    assert_eq!(
        server(&d.channel)
            .await
            .maintenance
            .unwrap()
            .compaction_count,
        1
    );
    d.stop().await;
}
