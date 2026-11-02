//! Replay sessions driven by the runtime, on a fake clock.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "../../query/tests/common/mod.rs"]
mod common;
mod replays_common;

use std::time::Duration;

use norn_core::Config;
use norn_replay::{ClockKind, Control, Pace, Payload};
use replays_common::{events, id, setup};

#[tokio::test(flavor = "multi_thread")]
async fn a_subscriber_sees_every_event_in_order_then_the_end() {
    let s = setup(500, Config::default());
    let id = id(&s, Pace::Maximum, 500);
    let mut sub = s.replays.subscribe(&id).await.unwrap();
    s.replays.control(&id, None, Control::Resume).await.unwrap();
    let got = events(&mut sub, 502).await;
    assert_eq!(got[0], Payload::Clock(ClockKind::Running));
    let times: Vec<i64> = got[1..501]
        .iter()
        .map(|p| match p {
            Payload::Sample { sample, .. } => sample.timestamp.as_nanos(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(times, (0..500).collect::<Vec<_>>());
    assert_eq!(got[501], Payload::Clock(ClockKind::Ended));
    s.cancel.cancel();
    s.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_replay_subscriber_slows_the_clock_and_misses_nothing() {
    let s = setup(5_000, Config::default());
    let id = id(&s, Pace::Maximum, 5_000);
    let mut slow = s.replays.subscribe(&id).await.unwrap();
    s.replays.control(&id, None, Control::Resume).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let ahead = s.replays.get(&id).unwrap().view.emitted;
    assert!(
        ahead < 5_000,
        "the clock ran ahead of a reader that read nothing"
    );
    let got = events(&mut slow, 5_002).await;
    let samples = got
        .iter()
        .filter(|p| matches!(p, Payload::Sample { .. }))
        .count();
    assert_eq!(samples, 5_000);
    s.cancel.cancel();
    s.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn real_time_follows_the_injected_clock() {
    let s = setup(10, Config::default());
    let id = id(&s, Pace::default(), 10);
    let mut sub = s.replays.subscribe(&id).await.unwrap();
    s.replays.control(&id, None, Control::Resume).await.unwrap();
    assert_eq!(
        events(&mut sub, 2).await.len(),
        2,
        "running, then the sample at 0"
    );
    // Two sleepers: the session table's expiry tick, and the driver waiting for
    // the sample at 1. Advancing sooner is also correct, since the driver sleeps
    // until a deadline (`Clock::sleep_until`), but then this would test less.
    while s.clock.sleepers() < 2 {
        tokio::task::yield_now().await;
    }
    s.clock.advance(Duration::from_nanos(4));
    let got = events(&mut sub, 4).await;
    assert!(
        got.iter().all(|p| matches!(p, Payload::Sample { .. })),
        "{got:?}"
    );
    s.cancel.cancel();
    s.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_controls_answer_already_shows_what_it_did() {
    let s = setup(100, Config::default());
    let id = id(&s, Pace::Maximum, 100);
    for d in 1..=20i64 {
        let seek = Control::Seek {
            to: norn_core::Timestamp::from_nanos(0),
            report_state: false,
        };
        let info = s.replays.control(&id, None, seek).await.unwrap();
        assert_eq!((info.current_time.as_nanos(), info.view.emitted), (0, 0));
        let step = Control::StepTime(Duration::from_nanos(d as u64));
        let info = s.replays.control(&id, None, step).await.unwrap();
        assert_eq!(
            info.current_time.as_nanos(),
            d,
            "the clock where the step left it"
        );
        assert_eq!(
            info.view.emitted,
            d as u64,
            "samples 0 to {} emitted",
            d - 1
        );
    }
    s.cancel.cancel();
    s.task.await.unwrap();
}
