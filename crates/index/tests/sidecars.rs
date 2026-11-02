//! Sidecars and crash residue: each is repaired from the segments, and counted.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{Scratch, brute, chunk, keys, open, plan, sealed, writer};
use norn_index::{IndexOptions, PlanRequest, StreamReport, TemporalIndex};

fn three_segments(s: &Scratch) {
    for seq in 0..3u64 {
        let chunks: Vec<_> = (0..5).map(|n| chunk(n, seq as i64 * 100, 10, 1)).collect();
        sealed(&s.dir(), seq, &chunks);
    }
}

fn reopened(s: &Scratch) -> StreamReport {
    let (_, report) = TemporalIndex::open(s.root(), IndexOptions::default()).unwrap();
    report.streams.into_iter().next().unwrap()
}

#[test]
fn a_crash_mid_update_leaves_nothing_a_startup_cannot_reconcile() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    fs::write(s.index_dir().join("summary.nidx.tmp"), b"torn").unwrap();
    fs::write(
        s.index_dir().join("0000000000000009-g0000.ndet.1-1.tmp"),
        b"x",
    )
    .unwrap();
    fs::write(s.sidecar(40, 0), b"orphan").unwrap();
    let r = reopened(&s);
    assert_eq!(r.orphans_removed, 3);
    assert!(r.clean());
}

#[test]
fn a_damaged_sidecar_is_answered_from_the_segment_and_repaired() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    let good = fs::read(s.sidecar(1, 0)).unwrap();
    for at in (0..good.len()).step_by(3) {
        let mut bad = good.clone();
        bad[at] ^= 0x10;
        fs::write(s.sidecar(1, 0), &bad).unwrap();
        let index = open(&s);
        let got = plan(&index.snapshot(), PlanRequest::everything());
        assert_eq!(keys(&got), keys(&brute(&s.dir())), "byte {at}");
        assert_eq!(index.stats().detail_repairs, 1, "byte {at}");
        assert_eq!(fs::read(s.sidecar(1, 0)).unwrap(), good, "byte {at}");
    }
}

#[test]
fn a_missing_sidecar_is_repaired_on_first_use() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    fs::remove_file(s.sidecar(2, 0)).unwrap();
    let index = open(&s);
    plan(&index.snapshot(), PlanRequest::everything());
    assert_eq!(index.stats().detail_repairs, 1);
    assert!(s.sidecar(2, 0).exists());
}

#[test]
fn an_unsealed_segment_found_at_startup_is_served_as_a_tail() {
    let s = Scratch::new();
    three_segments(&s);
    let mut w = writer(&s.dir(), 3);
    w.append(&chunk(1, 300, 10, 1)).unwrap();
    w.sync().unwrap();
    let r = reopened(&s);
    assert_eq!(r.tails, 1);
    drop(w);
}
