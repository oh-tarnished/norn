//! SPEC §36: record, store, replay gives the same events, every time, at any pace.
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

use std::time::Duration;

use common::oracle::*;
use common::{Scratch, target};
use drive::{Seen, Wall, run, samples, session};
use norn_core::{Sample, ValueKind};
use norn_replay::{ClockKind, Control, Pace};
use proptest::prelude::*;

fn expected(all: &[Sample], from: i64, to: i64) -> Vec<(String, i64, u64)> {
    let mut want: Vec<_> = all
        .iter()
        .filter(|s| (from..to).contains(&s.timestamp.as_nanos()))
        .map(|s| {
            let name = target(s.signal.get(), ValueKind::F64).name.to_string();
            (name, s.timestamp.as_nanos(), s.sequence.get())
        })
        .collect();
    want.sort_by(|a, b| (a.1, a.2, &a.0).cmp(&(b.1, b.2, &b.0)));
    want
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn a_replay_emits_exactly_the_stored_samples_in_order_at_any_pace(
        raw in raw(), shape in layout(), a in 0i64..400, b in 0i64..400, speed in 1u32..200,
    ) {
        let s = Scratch::new();
        let all = samples_of(&raw);
        let r = lay_out(&s, &rig0(shape), &all);
        let (from, to) = (a.min(b), a.max(b) + 1);
        let want = expected(&all, from, to);
        let mut wall = Wall(0);

        let mut fast = session(&r.engine, SIGNALS, from, to, Pace::Maximum);
        fast.control(Control::Resume, wall.now()).unwrap();
        let got = run(&mut fast, &mut wall, Duration::MAX);
        prop_assert_eq!(samples(&got), want.clone());
        prop_assert_eq!(got.last(), Some(&Seen::Clock(ClockKind::Ended, to)));

        // Paced, and interrupted by pauses: the same events, the same order.
        let pace = Pace::speed(f64::from(speed) / 10.0).unwrap();
        let mut slow = session(&r.engine, SIGNALS, from, to, pace);
        let mut events = Vec::new();
        for round in 0..1_000 {
            slow.control(Control::Resume, wall.now()).unwrap();
            events.extend(run(&mut slow, &mut wall, Duration::from_nanos(7)));
            if matches!(events.last(), Some(Seen::Clock(ClockKind::Ended, _))) {
                break;
            }
            prop_assert!(round < 999, "never ended");
            slow.control(Control::Pause, wall.now()).unwrap();
            wall.0 += 1_000;
        }
        prop_assert_eq!(samples(&events), want);
    }

    #[test]
    fn a_seek_resumes_at_its_target_and_reports_the_state_just_before_it(
        raw in raw(), shape in layout(), to in 0i64..400,
    ) {
        let s = Scratch::new();
        let all = samples_of(&raw);
        let r = lay_out(&s, &rig0(shape), &all);
        let mut wall = Wall(0);
        let mut replay = session(&r.engine, SIGNALS, 0, 400, Pace::Maximum);
        replay.control(Control::StepEvents(5), wall.now()).unwrap();
        run(&mut replay, &mut wall, Duration::MAX);
        let seek = Control::Seek { to: common::at(to), report_state: true };
        let batches = replay.control(seek, wall.now()).unwrap();
        let reported = drive::seen(batches);
        prop_assert_eq!(&reported[0], &Seen::Clock(ClockKind::Seeked, to));
        let mut want = Vec::new();
        for signal in 0..SIGNALS {
            let before = all.iter().filter(|x| x.signal.get() == signal && x.timestamp.as_nanos() < to);
            if let Some(x) = before.max_by_key(|x| key_time(x)) {
                want.push(Seen::State(target(signal, ValueKind::F64).name.to_string(), x.clone()));
            }
        }
        prop_assert_eq!(&reported[1..], &want[..]);
        prop_assert_eq!(replay.view().emitted, 0);
        replay.control(Control::Resume, wall.now()).unwrap();
        let rest = run(&mut replay, &mut wall, Duration::MAX);
        prop_assert_eq!(samples(&rest), expected(&all, to, 400));
    }
}

fn samples_of(raw: &[Raw]) -> Vec<Sample> {
    common::oracle::samples(raw)
}
