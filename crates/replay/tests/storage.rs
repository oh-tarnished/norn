//! Replay over storage that moves or breaks underneath it.
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

use std::io::{Seek, SeekFrom, Write};
use std::time::Duration;

use common::{STREAM, Scratch, rig, sample, store};
use drive::{Wall, run, samples, session};
use norn_core::Value;
use norn_query::{QueryError, QueryLimits};
use norn_replay::{Control, Next, Pace, ReplayError};
use norn_storage::{StorageOptions, compact, live_segments};

#[test]
fn compaction_in_the_middle_of_a_replay_loses_and_doubles_nothing() {
    let s = Scratch::new();
    let segments: Vec<Vec<_>> = (0..6)
        .map(|seg| {
            let row = |sig| (0..50).map(move |t| sample(sig, seg * 100 + t, Value::F64(t as f64)));
            (0..3).flat_map(row).collect()
        })
        .collect();
    store(&s.dir(), &segments, 10);
    let mut limits = QueryLimits::default();
    limits.page_results = 16;
    limits.slice_bytes = 4_096;
    let r = rig(&s.0, 1 << 30, limits);
    let mut wall = Wall(0);
    let mut whole = session(&r.engine, 3, 0, 1_000, Pace::Maximum);
    whole.control(Control::Resume, wall.now()).unwrap();
    let want = samples(&run(&mut whole, &mut wall, Duration::MAX));
    assert_eq!(want.len(), 900);

    let mut replay = session(&r.engine, 3, 0, 1_000, Pace::Maximum);
    let (mut got, mut compactions) = (Vec::new(), 0);
    for round in 0.. {
        replay.control(Control::StepEvents(97), wall.now()).unwrap();
        let step = run(&mut replay, &mut wall, Duration::MAX);
        got.extend(samples(&step));
        if replay.view().state == norn_replay::ReplayState::Ended {
            break;
        }
        let live = live_segments(&s.dir()).unwrap();
        if round % 2 == 0 && live.len() > 1 {
            let inputs: Vec<_> = live.iter().take(2).map(|e| e.path.clone()).collect();
            let outcome = compact(&s.dir(), &inputs, &StorageOptions::default()).unwrap();
            r.index.compaction_applied(STREAM, &outcome).unwrap();
            compactions += 1;
        }
    }
    assert!(compactions >= 2, "the test did not compact");
    assert_eq!(got, want);
}

#[test]
fn a_damaged_chunk_fails_the_read_and_pauses_before_it() {
    let s = Scratch::new();
    store(
        &s.dir(),
        &[(0..10).map(|t| sample(0, t, Value::F64(1.0))).collect()],
        10,
    );
    let path = live_segments(&s.dir()).unwrap()[0].path.clone();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(100)).unwrap();
    file.write_all(&[0xff; 16]).unwrap();
    drop(file);
    let mut replay = session(&r.engine, 1, 0, 100, Pace::Maximum);
    let wall = common::at(0);
    replay.control(Control::Resume, wall).unwrap();
    assert!(matches!(replay.poll(wall), Next::Read));
    let failed = replay.fill(wall).unwrap_err();
    let ReplayError::Query(e) = failed else {
        panic!("{failed:?}")
    };
    assert!(matches!(*e, QueryError::Corrupt(_)), "{e:?}");
    assert_eq!(replay.view().state, norn_replay::ReplayState::Paused);
    assert_eq!(replay.view().emitted, 0);
}
