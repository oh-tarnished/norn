//! The clock controls, on a clock the test moves.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "../../query/tests/common/mod.rs"]
mod common;
mod drive;
mod ten;

use std::time::Duration;

use common::at;
use drive::{Wall, run, session};
use norn_replay::{Control, Next, Pace, ReplayError, ReplayState};
use ten::{SECOND, ten, times};

#[test]
fn a_new_session_is_paused_and_emits_nothing_until_told() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::default());
    assert_eq!(replay.view().state, ReplayState::Paused);
    assert!(matches!(replay.poll(at(5 * SECOND)), Next::Idle));
}

#[test]
fn a_step_emits_exactly_its_events_and_stays_paused() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::default());
    let mut wall = Wall(0);
    replay.control(Control::StepEvents(3), wall.now()).unwrap();
    assert_eq!(
        times(&run(&mut replay, &mut wall, Duration::MAX)),
        [0, 1, 2]
    );
    assert_eq!(replay.view().state, ReplayState::Paused);
    assert_eq!(replay.view().clock.now(wall.now()), at(2 * SECOND));
    replay
        .control(Control::StepTime(Duration::from_millis(2_500)), wall.now())
        .unwrap();
    assert_eq!(times(&run(&mut replay, &mut wall, Duration::MAX)), [3, 4]);
    assert_eq!(replay.view().clock.now(wall.now()), at(4_500_000_000));
    assert_eq!(wall.0, 0, "a step takes no wall time");
    replay
        .control(Control::StepEvents(100), wall.now())
        .unwrap();
    let rest = run(&mut replay, &mut wall, Duration::MAX);
    assert_eq!(
        times(&rest),
        [5, 6, 7, 8, 9],
        "fewer, since the interval ended"
    );
    assert_eq!(replay.view().state, ReplayState::Ended);
}

#[test]
fn a_seek_from_ended_pauses_and_a_seek_outside_the_interval_is_refused() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::Maximum);
    let mut wall = Wall(0);
    replay.control(Control::Resume, wall.now()).unwrap();
    run(&mut replay, &mut wall, Duration::MAX);
    assert_eq!(replay.view().state, ReplayState::Ended);
    let outside = Control::Seek {
        to: at(11 * SECOND),
        report_state: false,
    };
    assert!(matches!(
        replay.control(outside, wall.now()),
        Err(ReplayError::OutsideInterval)
    ));
    let back = Control::Seek {
        to: at(7 * SECOND),
        report_state: false,
    };
    replay.control(back, wall.now()).unwrap();
    assert_eq!(replay.view().state, ReplayState::Paused);
    replay.control(Control::StepEvents(1), wall.now()).unwrap();
    assert_eq!(times(&run(&mut replay, &mut wall, Duration::MAX)), [7]);
}

#[test]
fn a_stopped_session_refuses_everything_but_stop() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::default());
    let wall = at(0);
    replay.control(Control::Stop, wall).unwrap();
    replay.control(Control::Stop, wall).unwrap();
    for control in [
        Control::Resume,
        Control::StepEvents(1),
        Control::SetPace(Pace::Maximum),
    ] {
        let refused = replay.control(control, wall).unwrap_err();
        assert!(
            matches!(
                refused,
                ReplayError::Conflict {
                    state: "stopped",
                    ..
                }
            ),
            "{control:?}"
        );
    }
    let ended = Control::Resume;
    let mut done = session(&r.engine, 1, 0, 0, Pace::Maximum);
    done.control(ended, wall).unwrap();
    run(&mut done, &mut Wall(0), Duration::MAX);
    assert!(matches!(
        done.control(Control::Resume, wall),
        Err(ReplayError::Conflict { .. })
    ));
}

#[test]
fn steps_need_a_paused_or_frozen_clock() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::default());
    replay.control(Control::Resume, at(0)).unwrap();
    let refused = replay.control(Control::StepEvents(1), at(0)).unwrap_err();
    assert!(matches!(
        refused,
        ReplayError::Conflict {
            state: "running",
            ..
        }
    ));
}
