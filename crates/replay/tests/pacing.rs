//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Pace: real time, multiples of it, zero, and pausing, on a clock the test moves.
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
use drive::{Seen, Wall, run, session};
use norn_replay::{ClockKind, Control, Next, Pace, ReplayState};
use ten::{SECOND, ten, times};

#[test]
fn at_real_time_a_sample_comes_due_when_the_wall_clock_reaches_it() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::default());
    let mut wall = Wall(100 * SECOND);
    let started = replay.control(Control::Resume, wall.now()).unwrap();
    assert_eq!(drive::seen(started), [Seen::Clock(ClockKind::Running, 0)]);
    let mut got = Vec::new();
    while got.len() < 3 {
        match replay.poll(wall.now()) {
            Next::Emit(b) => got.extend(drive::seen([b])),
            Next::Read => replay.fill(wall.now()).unwrap(),
            Next::Wait(d) => {
                assert!(d <= Duration::from_secs(1), "{d:?}");
                wall.0 += d.as_nanos() as i64;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(times(&got), [0, 1, 2]);
    assert_eq!(
        wall.0,
        102 * SECOND,
        "the third sample is due two seconds in"
    );
}

#[test]
fn ten_times_speed_plays_ten_seconds_in_one() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::speed(10.0).unwrap());
    let mut wall = Wall(0);
    replay.control(Control::Resume, wall.now()).unwrap();
    let got = run(&mut replay, &mut wall, Duration::MAX);
    assert_eq!(times(&got).len(), 10);
    assert_eq!(
        wall.0, SECOND,
        "ended when the clock reached the interval's end"
    );
    assert_eq!(
        got.last(),
        Some(&Seen::Clock(ClockKind::Ended, 10 * SECOND))
    );
    assert_eq!(replay.view().state, ReplayState::Ended);
}

#[test]
fn pausing_freezes_the_clock_and_resuming_carries_on_from_there() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::default());
    let mut wall = Wall(0);
    replay.control(Control::Resume, wall.now()).unwrap();
    wall.0 = 3 * SECOND + 1;
    let paused = replay.control(Control::Pause, wall.now()).unwrap();
    assert_eq!(
        drive::seen(paused),
        [Seen::Clock(ClockKind::Paused, 3 * SECOND + 1)]
    );
    let got = run(&mut replay, &mut wall, Duration::MAX);
    assert!(got.is_empty(), "nothing is due while paused: {got:?}");
    wall.0 += 3_600 * SECOND;
    assert_eq!(replay.view().clock.now(wall.now()), at(3 * SECOND + 1));
    replay.control(Control::Resume, wall.now()).unwrap();
    let got = run(&mut replay, &mut wall, Duration::from_millis(1));
    assert_eq!(times(&got), (0..10).collect::<Vec<_>>());
}

#[test]
fn at_zero_speed_the_clock_moves_only_by_steps() {
    let (_s, r) = ten();
    let mut replay = session(&r.engine, 1, 0, 10 * SECOND, Pace::speed(0.0).unwrap());
    let mut wall = Wall(0);
    replay.control(Control::Resume, wall.now()).unwrap();
    wall.0 += 1_000 * SECOND;
    assert!(run(&mut replay, &mut wall, Duration::MAX).is_empty());
    replay.control(Control::StepEvents(2), wall.now()).unwrap();
    assert_eq!(times(&run(&mut replay, &mut wall, Duration::MAX)), [0, 1]);
    assert_eq!(replay.view().state, ReplayState::Running);
}
