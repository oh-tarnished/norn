//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Replay's controls and refusals over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod events_common;

use common::{append, batch, double, seed, start, temp};
use events_common::{create, kind, replay, replays, sample, seconds, subscribe, until};
use norn_protocol::common::ValueType::Double;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::step_replay_session_request::Step;
use norn_protocol::replay::{
    ReplaySession, ReplayState, ResumeReplaySessionRequest, SeekReplaySessionRequest,
    StepReplaySessionRequest, UpdateReplaySessionRequest,
};
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_protocol::telemetry::{ClockEventKind, event};
use norn_server::DaemonConfig;

#[tokio::test(flavor = "multi_thread")]
async fn step_seek_and_pace_move_the_clock_and_tell_subscribers() {
    let d = start(DaemonConfig::default(), temp("replay-controls")).await;
    seed(&d.channel, &[("a", Double), ("b", Double)]).await;
    let list = (0..20)
        .flat_map(|n| [double("a", n), double("b", n)])
        .collect();
    append(&d.channel, vec![batch(1, list, Persisted)]).await;
    let session = create(&d.channel, 1_000, 1_020, Pace::Speed(1.0)).await;
    let name = session.name.clone();
    let mut events = subscribe(&d.channel, replay(&name)).await.unwrap();
    let mut client = replays(&d.channel);

    let step = |s| StepReplaySessionRequest {
        name: name.clone(),
        step: Some(s),
        etag: String::new(),
    };
    let after = client
        .step_replay_session(step(Step::EventCount(3)))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        (after.state, after.emitted_count),
        (ReplayState::Paused as i32, 3)
    );
    let got = until(&mut events, 0, |e| e.event_sequence == 3).await;
    assert!(got.iter().all(|e| sample(e).is_some()));

    let seek = SeekReplaySessionRequest {
        name: name.clone(),
        seek_time: seconds(1_010),
        etag: String::new(),
    };
    let sought = client.seek_replay_session(seek).await.unwrap().into_inner();
    assert_eq!(
        (sought.emitted_count, sought.current_time),
        (0, seconds(1_010))
    );
    let got = until(&mut events, 3, |e| e.event_sequence == 6).await;
    assert_eq!(kind(&got[0]), Some(ClockEventKind::Seeked));
    let held: Vec<_> = got[1..]
        .iter()
        .map(|e| match &e.payload {
            Some(event::Payload::StateSample(s)) => (s.signal.clone(), s.sequence),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        held,
        [
            ("entities/dev/signals/a".into(), 9),
            ("entities/dev/signals/b".into(), 9)
        ]
    );

    let duration = prost_types::Duration {
        seconds: 2,
        nanos: 0,
    };
    client
        .step_replay_session(step(Step::Duration(duration)))
        .await
        .unwrap();
    let got = until(&mut events, 6, |e| e.event_sequence == 10).await;
    let seqs: Vec<_> = got.iter().map(|e| sample(e).unwrap().sequence).collect();
    assert_eq!(seqs, [10, 10, 11, 11]);

    let fast = ReplaySession {
        name: name.clone(),
        pace: Some(Pace::MaxSpeed(true)),
        ..ReplaySession::default()
    };
    let update = UpdateReplaySessionRequest {
        replay_session: Some(fast),
        update_mask: None,
    };
    client.update_replay_session(update).await.unwrap();
    let resume = ResumeReplaySessionRequest {
        name: name.clone(),
        etag: String::new(),
    };
    client.resume_replay_session(resume).await.unwrap();
    let got = until(&mut events, 10, |e| kind(e) == Some(ClockEventKind::Ended)).await;
    assert_eq!(kind(&got[0]), Some(ClockEventKind::SpeedChanged));
    assert_eq!(kind(&got[1]), Some(ClockEventKind::Running));
    assert_eq!(got.iter().filter(|e| sample(e).is_some()).count(), 16);
    let _ = std::fs::remove_dir_all(d.stop_keeping().await);
}
