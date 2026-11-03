//! `DriveReplaySession` over real gRPC: a step returns exactly what a time-ordered
//! read gives for the same stretch, a seek returns the state, and every bound is
//! refused before the clock moves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "events_common/mod.rs"]
mod events_common;
#[path = "query_common/mod.rs"]
mod query_common;
#[path = "reads_common/mod.rs"]
mod reads_common;

use common::{start, temp};
use events_common::{create, replays};
use norn_protocol::common::Sample;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{BrowseSamplesRequest, SampleOrder};
use norn_protocol::replay::drive_replay_session_request::Movement;
use norn_protocol::replay::replay_event::Payload;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{DriveReplaySessionRequest, DriveReplaySessionResponse, ReplayState};
use norn_server::DaemonConfig;
use query_common::reason;
use reads_common::{at, query, second, seed};
use tonic::transport::Channel;

fn drive(name: &str, movement: Movement) -> DriveReplaySessionRequest {
    DriveReplaySessionRequest {
        name: name.into(),
        movement: Some(movement),
        max_events: 1000,
        max_value_bytes: 65_536,
        ..DriveReplaySessionRequest::default()
    }
}

async fn go(c: &Channel, r: DriveReplaySessionRequest) -> DriveReplaySessionResponse {
    replays(c)
        .drive_replay_session(r)
        .await
        .unwrap()
        .into_inner()
}

fn samples(r: &DriveReplaySessionResponse) -> Vec<Sample> {
    let each = r
        .replay_events
        .iter()
        .filter_map(|e| match e.payload.as_ref() {
            Some(Payload::Sample(s)) => Some(s.clone()),
            _ => None,
        });
    each.collect()
}

/// `entities/ahu`'s samples in [from, to), in replay order.
async fn by_time(c: &Channel, from: i64, to: i64) -> Vec<Sample> {
    let r = BrowseSamplesRequest {
        parent: "entities/ahu".into(),
        interval: Some(Interval {
            start_time: at(from),
            end_time: at(to),
        }),
        order: SampleOrder::ByTime as i32,
        page_size: 1000,
        max_value_bytes: 65_536,
        ..BrowseSamplesRequest::default()
    };
    query(c)
        .browse_samples(r)
        .await
        .unwrap()
        .into_inner()
        .samples
}

#[tokio::test(flavor = "multi_thread")]
async fn steps_return_exactly_the_time_ordered_read_and_a_seek_returns_state() {
    let d = start(DaemonConfig::default(), temp("drive")).await;
    seed(&d.channel).await;
    let c = &d.channel;
    let name = create(c, 1_000, 1_030, Pace::Speed(0.0)).await.name;
    let stepped = go(c, drive(&name, Movement::EventCount(7))).await;
    let want = by_time(c, 1_000, 1_030).await;
    assert_eq!(samples(&stepped), want[..7]);
    let seconds = prost_types::Duration {
        seconds: 5,
        nanos: 0,
    };
    let stepped = go(c, drive(&name, Movement::Duration(seconds))).await;
    let clock = second(&want[6]);
    let rest: Vec<Sample> = want[7..]
        .iter()
        .filter(|s| second(s) < clock + 5)
        .cloned()
        .collect();
    assert_eq!(samples(&stepped), rest);
    let sought = go(c, drive(&name, Movement::SeekTime(at(1_020).unwrap()))).await;
    let states: Vec<&Sample> = sought
        .replay_events
        .iter()
        .filter_map(|e| match e.payload.as_ref() {
            Some(Payload::StateSample(s)) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        states.len(),
        3,
        "temp, on and mode each had a sample before 1020"
    );
    assert!(states.iter().all(|s| second(s) == 1_019));
    let ended = go(c, drive(&name, Movement::EventCount(1000))).await;
    assert_eq!(samples(&ended), by_time(c, 1_020, 1_030).await);
    let session = ended.replay_session.unwrap();
    assert_eq!(session.state, ReplayState::Ended as i32);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_movement_too_big_is_refused_before_the_clock_moves() {
    let d = start(DaemonConfig::default(), temp("drive-bounds")).await;
    seed(&d.channel).await;
    let c = &d.channel;
    let name = create(c, 1_000, 1_400, Pace::Speed(0.0)).await.name;
    let mut r = drive(&name, Movement::EventCount(11));
    r.max_events = 10;
    let refused = replays(c).drive_replay_session(r).await.unwrap_err();
    assert_eq!(reason(&refused), "ERROR_REASON_LIMIT_EXCEEDED");
    let mut r = drive(
        &name,
        Movement::Duration(prost_types::Duration {
            seconds: 100,
            nanos: 0,
        }),
    );
    r.max_events = 10;
    let refused = replays(c).drive_replay_session(r).await.unwrap_err();
    assert_eq!(reason(&refused), "ERROR_REASON_LIMIT_EXCEEDED");
    let first = go(c, drive(&name, Movement::EventCount(1))).await;
    assert_eq!(second(&samples(&first)[0]), 1_000, "nothing moved before");
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_frozen_session_is_driven() {
    let d = start(DaemonConfig::default(), temp("drive-frozen")).await;
    seed(&d.channel).await;
    let c = &d.channel;
    let name = create(c, 1_000, 1_400, Pace::Speed(1.0)).await.name;
    let refused = replays(c)
        .drive_replay_session(drive(&name, Movement::EventCount(1)))
        .await;
    assert_eq!(
        reason(&refused.unwrap_err()),
        "ERROR_REASON_REPLAY_STATE_CONFLICT"
    );
    let frozen = create(c, 1_000, 1_400, Pace::Speed(0.0)).await.name;
    let mut r = drive(&frozen, Movement::EventCount(1));
    r.movement = None;
    let refused = replays(c).drive_replay_session(r).await.unwrap_err();
    assert_eq!(reason(&refused), "ERROR_REASON_INVALID_ARGUMENT");
    d.stop().await;
}
