//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery after `kill -9`: new samples land after the compacted history, the
//! daemon is killed without warning, and the restart is timed from spawn to its
//! first answer. The daemon's own `RecoveryReport` says what it had to do.

use std::sync::Arc;

use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;

use crate::daemon::Daemon;
use crate::ingest::EPOCH;
use crate::load;
use crate::profile::History;
use crate::stats::{Better, Metrics};

pub async fn crash(daemon: &mut Daemon, names: &Arc<Vec<String>>, h: &History, m: &mut Metrics) {
    load::load(&daemon.channel, names, EPOCH + h.seconds, h.crash_seconds).await;
    daemon.kill9();
    let took = daemon.restart().await.as_secs_f64();
    let get = GetServerRequest {
        name: "server".into(),
    };
    let server = AdminServiceClient::new(daemon.channel.clone())
        .get_server(get)
        .await
        .unwrap()
        .into_inner();
    let report = server.recovery.unwrap_or_default();
    assert_eq!(
        report.corruption_count, 0,
        "recovery found corruption: {report:?}"
    );
    m.record("history.recovery.restart_s", "s", Better::Lower, true, took);
    let replayed = report.replayed_sample_count as f64;
    m.record(
        "history.recovery.replayed_samples",
        "samples",
        Better::Lower,
        false,
        replayed,
    );
    let verified = report.verified_segment_count as f64;
    m.record(
        "history.recovery.verified_segments",
        "segments",
        Better::Lower,
        false,
        verified,
    );
    println!("recovery after kill -9: {took:.2} s, {report:?}");
}
