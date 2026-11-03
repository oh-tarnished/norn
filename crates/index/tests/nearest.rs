//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Timestamp → nearest chunk at or before, at every edge.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, chunk, open, plan, sealed, secs};
use norn_core::{SignalId, Timestamp};
use norn_index::{PlanRequest, SignalFilter, Snapshot};
use norn_storage::{ChunkFile, StorageOptions};

const SIGNAL: SignalId = SignalId::new(3);

fn near(s: &Snapshot, t: i64) -> Option<(i64, i64)> {
    let l = s.nearest_at_or_before(SIGNAL, secs(t)).unwrap()?;
    Some((
        l.min_ts().as_nanos() / 1_000_000_000,
        l.max_ts().as_nanos() / 1_000_000_000,
    ))
}

/// Signal 3 in 100..=109 and 200..=209 with a gap; signal 4 fills the gap.
fn gapped(s: &Scratch) {
    sealed(&s.dir(), 0, &[chunk(3, 100, 10, 1), chunk(4, 100, 10, 1)]);
    sealed(&s.dir(), 1, &[chunk(4, 150, 10, 1)]);
    sealed(&s.dir(), 2, &[chunk(3, 200, 10, 1), chunk(4, 200, 10, 1)]);
}

#[test]
fn before_the_first_chunk_there_is_nothing() {
    let s = Scratch::new();
    gapped(&s);
    let index = open(&s);
    assert_eq!(near(&index.snapshot(), 99), None);
}

#[test]
fn exactly_on_the_first_and_last_sample_finds_that_chunk() {
    let s = Scratch::new();
    gapped(&s);
    let index = open(&s);
    let snap = index.snapshot();
    assert_eq!(near(&snap, 100), Some((100, 109)));
    assert_eq!(near(&snap, 109), Some((100, 109)));
    assert_eq!(near(&snap, 200), Some((200, 209)));
}

#[test]
fn inside_a_gap_finds_the_chunk_before_it() {
    let s = Scratch::new();
    gapped(&s);
    let index = open(&s);
    let snap = index.snapshot();
    assert_eq!(near(&snap, 110), Some((100, 109)));
    assert_eq!(
        near(&snap, 199),
        Some((100, 109)),
        "past another signal's segment"
    );
}

#[test]
fn the_future_finds_the_last_chunk() {
    let s = Scratch::new();
    gapped(&s);
    let index = open(&s);
    let snap = index.snapshot();
    assert_eq!(near(&snap, 1_000_000), Some((200, 209)));
    let max = snap
        .nearest_at_or_before(SIGNAL, Timestamp::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(max.min_ts(), secs(200));
}

#[test]
fn an_unknown_signal_and_an_empty_database_find_nothing() {
    let s = Scratch::new();
    assert_eq!(near(&open(&s).snapshot(), 100), None);
    sealed(&s.dir(), 0, &[chunk(4, 100, 10, 1)]);
    assert_eq!(near(&open(&s).snapshot(), 200), None);
}

#[test]
fn an_empty_segment_is_passed_over() {
    let s = Scratch::new();
    sealed(&s.dir(), 0, &[chunk(3, 100, 10, 1)]);
    sealed(&s.dir(), 1, &[]);
    let index = open(&s);
    assert_eq!(
        near(&index.snapshot(), i64::from(i32::MAX)),
        Some((100, 109))
    );
}

#[test]
fn a_chunk_that_ends_closer_beats_one_that_started_later() {
    let s = Scratch::new();
    sealed(&s.dir(), 0, &[chunk(3, 100, 2, 100)]); // 100, 200
    sealed(&s.dir(), 1, &[chunk(3, 120, 2, 10)]); // 120, 130
    let index = open(&s);
    assert_eq!(near(&index.snapshot(), 150), Some((100, 200)));
}

/// What StateAt does: read the nearest chunk, then plan the rest of the way.
fn state_at(s: &std::sync::Arc<Snapshot>, t: i64) -> Option<i64> {
    let read = |l: &norn_index::ChunkLocator, t: i64| {
        let mut f = ChunkFile::open(&s.path_of(l), StorageOptions::default()).unwrap();
        let chunk = f.read_chunk(&l.chunk).unwrap();
        chunk
            .samples()
            .map(|x| x.timestamp)
            .filter(|x| *x <= secs(t))
            .max()
    };
    let nearest = s.nearest_at_or_before(SIGNAL, secs(t)).unwrap()?;
    let found = read(&nearest, t).expect("the nearest chunk holds a sample at or before t");
    let rest = PlanRequest::new(found.checked_add_nanos(1).unwrap(), secs(t))
        .signals(SignalFilter::one(SIGNAL));
    let best = plan(s, rest)
        .iter()
        .filter_map(|l| read(l, t))
        .max()
        .unwrap_or(found);
    Some(best.as_nanos() / 1_000_000_000)
}

#[test]
fn the_two_step_finds_the_true_sample_under_overlap() {
    let s = Scratch::new();
    // A sparse chunk straddles a dense one; at 155 the true answer is 150, in
    // the dense chunk, though the sparse one ends later.
    sealed(&s.dir(), 0, &[chunk(3, 100, 3, 100)]); // 100, 200, 300
    sealed(&s.dir(), 1, &[chunk(3, 140, 2, 10)]); // 140, 150
    let index = open(&s);
    let snap = index.snapshot();
    assert_eq!(near(&snap, 155), Some((100, 300)));
    assert_eq!(state_at(&snap, 155), Some(150));
    assert_eq!(state_at(&snap, 250), Some(200));
    assert_eq!(state_at(&snap, 99), None);
}
