//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Retention: nothing by default, nothing on a dry run, whole segments otherwise.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod query_common;
mod upkeep_common;

use std::path::PathBuf;
use std::time::Duration;

use common::{Running, append, batch, double, seed, start, temp};
use norn_core::{Config, MaintenanceConfig};
use norn_protocol::common::ValueType;
use norn_protocol::stream::retention_policy::Limit;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{RetentionPolicy, Stream, UpdateStreamRequest};
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_server::DaemonConfig;
use query_common::{all, samples};
use upkeep_common::{PLANT, retention, segments, server, stats, stream};

/// Three sealed segments: sequences 0..10, 1000..1010 and 2000..2010, each second.
async fn three_segments(tag: &str) -> PathBuf {
    let d = start(DaemonConfig::default(), temp(tag)).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let mut data = d.stop_keeping().await;
    for from in [0, 1000, 2000] {
        let d = start(DaemonConfig::default(), data).await;
        let list = (from..from + 10).map(|n| double("t", n)).collect();
        append(&d.channel, vec![batch(1, list, Durable)]).await;
        data = d.stop_keeping().await;
    }
    data
}

async fn set_policy(d: &Running, limit: Limit) {
    let request = UpdateStreamRequest {
        stream: Some(Stream {
            name: PLANT.into(),
            retention: Some(RetentionPolicy { limit: Some(limit) }),
            ..Stream::default()
        }),
        update_mask: Some(prost_types::FieldMask {
            paths: vec!["retention".into()],
        }),
        validate_only: false,
    };
    let mut client = StreamServiceClient::new(d.channel.clone());
    client.update_stream(request).await.unwrap();
}

fn seconds(seconds: i64) -> prost_types::Duration {
    prost_types::Duration { seconds, nanos: 0 }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_with_no_policy_keeps_everything() {
    let d = start(DaemonConfig::default(), three_segments("ret-none").await).await;
    let done = retention(&d.channel, false).await;
    assert_eq!(done.removed_segment_count, 0);
    assert_eq!(samples(&d.channel, all()).await.len(), 30);
    assert_eq!(segments(&d.data).len(), 3);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_says_what_would_go_and_removes_nothing_and_applying_removes_exactly_that() {
    let d = start(DaemonConfig::default(), three_segments("ret-age").await).await;
    set_policy(&d, Limit::MaxAge(seconds(500))).await;
    assert!(stream(&d.channel).await.retention.is_some());
    let before = stats(&d.channel).await;

    let dry = retention(&d.channel, true).await;
    assert!(!dry.applied);
    assert_eq!(dry.removed_segment_count, 2);
    assert_eq!(dry.removed_sample_count, 20);
    assert_eq!(segments(&d.data).len(), 3, "a dry run deleted something");
    assert_eq!(samples(&d.channel, all()).await.len(), 30);
    assert_eq!(
        server(&d.channel)
            .await
            .maintenance
            .unwrap()
            .retention_segment_count,
        0
    );

    let done = retention(&d.channel, false).await;
    assert!(done.applied);
    assert_eq!(done.removed_segments, dry.removed_segments);
    assert_eq!(segments(&d.data).len(), 1);
    let left = samples(&d.channel, all()).await;
    assert_eq!(left.len(), 10);
    assert!(left.iter().all(|s| s.sequence >= 2000));

    let after = stats(&d.channel).await;
    assert_eq!(after.sample_count, 10);
    assert_eq!(after.stored_bytes, before.stored_bytes - done.removed_bytes);
    assert_eq!(after.newest_sample_time, before.newest_sample_time);
    let oldest = after.oldest_sample_time.unwrap().seconds;
    assert_eq!(oldest, 3000);
    let report = server(&d.channel).await.maintenance.unwrap();
    assert_eq!(report.retention_segment_count, 2);
    assert_eq!(report.retention_sample_count, 20);
    assert_eq!(report.recent_removals[0].stream, PLANT);
    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::default(), data).await;
    assert_eq!(stats(&d.channel).await.sample_count, 10);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_size_limit_removes_the_oldest_whole_segments_and_never_the_newest() {
    let d = start(DaemonConfig::default(), three_segments("ret-size").await).await;
    set_policy(&d, Limit::MaxBytes(1)).await;
    let done = retention(&d.channel, false).await;
    assert_eq!(done.removed_segment_count, 2);
    assert_eq!(segments(&d.data).len(), 1);
    assert_eq!(samples(&d.channel, all()).await.len(), 10);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recorded_policy_runs_by_itself_on_the_schedule() {
    let data = three_segments("ret-sched").await;
    let d = start(DaemonConfig::default(), data).await;
    set_policy(&d, Limit::MaxAge(seconds(500))).await;
    let data = d.stop_keeping().await;
    let mut schedule = MaintenanceConfig::default();
    schedule.retention_interval = Duration::from_millis(20);
    let engine = Config::builder().maintenance(schedule).build().unwrap();
    let d = start(DaemonConfig::new(engine), data).await;
    for _ in 0..500 {
        let report = server(&d.channel).await.maintenance.unwrap();
        if report.retention_segment_count > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(segments(&d.data).len(), 1);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_policy_of_months_is_recorded_and_outlives_a_restart() {
    let d = start(DaemonConfig::default(), three_segments("ret-months").await).await;
    set_policy(&d, Limit::MaxMonths(6)).await;
    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::default(), data).await;
    let policy = stream(&d.channel).await.retention.unwrap();
    assert_eq!(policy.limit, Some(Limit::MaxMonths(6)));
    assert_eq!(retention(&d.channel, true).await.removed_segment_count, 0);
    d.stop().await;
}
