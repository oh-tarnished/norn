//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! ReplayService and REPLAY subscriptions over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod events_common;
#[path = "query_common/mod.rs"]
mod query_common;

use common::{append, batch, double, seed, start, temp};
use events_common::{create, kind, replay, replays, sample, seconds, subscribe, until};
use norn_protocol::common::ValueType::Double;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{QuerySamplesRequest, SampleOrder};
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{ReplayState, ResumeReplaySessionRequest, StopReplaySessionRequest};
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_protocol::telemetry::ClockEventKind;
use norn_server::DaemonConfig;
use query_common::samples;

const SIGNALS: [(&str, norn_protocol::common::ValueType); 3] =
    [("a", Double), ("b", Double), ("c", Double)];

/// 300 samples over three signals, at 1000 s to 1099 s, with equal times.
async fn load(c: &tonic::transport::Channel) {
    seed(c, &SIGNALS).await;
    let list = (0..300)
        .map(|n| double(["a", "b", "c"][n as usize % 3], n / 3))
        .collect();
    append(c, vec![batch(1, list, Persisted)]).await;
}

async fn play_all(c: &tonic::transport::Channel) -> Vec<norn_protocol::telemetry::Event> {
    let session = create(c, 1_000, 1_100, Pace::MaxSpeed(true)).await;
    assert_eq!(session.state, ReplayState::Paused as i32);
    let mut events = subscribe(c, replay(&session.name)).await.unwrap();
    let name = session.name.clone();
    let resume = ResumeReplaySessionRequest {
        name,
        etag: String::new(),
    };
    replays(c).resume_replay_session(resume).await.unwrap();
    until(&mut events, 0, |e| kind(e) == Some(ClockEventKind::Ended)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replay_gives_back_exactly_what_was_recorded_in_order_and_again() {
    let d = start(DaemonConfig::default(), temp("replay-equal")).await;
    load(&d.channel).await;
    let first = play_all(&d.channel).await;
    assert_eq!(kind(&first[0]), Some(ClockEventKind::Running));
    let got: Vec<_> = first.iter().filter_map(sample).cloned().collect();
    let stored = samples(
        &d.channel,
        QuerySamplesRequest {
            parent: "entities/dev".into(),
            interval: Some(Interval {
                start_time: seconds(1_000),
                end_time: seconds(1_100),
            }),
            order: SampleOrder::ByTime as i32,
            ..QuerySamplesRequest::default()
        },
    )
    .await;
    assert_eq!(got.len(), 300);
    assert_eq!(
        got, stored,
        "record, store, replay: the same samples, the same order"
    );
    for (e, s) in first.iter().filter(|e| sample(e).is_some()).zip(&got) {
        assert_eq!(
            e.clock_time, s.observe_time,
            "a sample event is timed by its sample"
        );
    }
    let second = play_all(&d.channel).await;
    let strip = |list: &[norn_protocol::telemetry::Event]| {
        list.iter()
            .map(|e| (e.event_sequence, e.payload.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&first), strip(&second), "deterministic across runs");
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_sends_stopped_last_and_closes_the_stream() {
    let d = start(DaemonConfig::default(), temp("replay-stop")).await;
    load(&d.channel).await;
    let session = create(&d.channel, 1_000, 1_100, Pace::Speed(1.0)).await;
    let mut events = subscribe(&d.channel, replay(&session.name)).await.unwrap();
    let stop = StopReplaySessionRequest {
        name: session.name.clone(),
        etag: String::new(),
    };
    let stopped = replays(&d.channel)
        .stop_replay_session(stop)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stopped.state, ReplayState::Stopped as i32);
    let got = until(&mut events, 0, |e| kind(e) == Some(ClockEventKind::Stopped)).await;
    assert_eq!(got.len(), 1);
    assert!(
        events.message().await.unwrap().is_none(),
        "closed after STOPPED"
    );
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}
