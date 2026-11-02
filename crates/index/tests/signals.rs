//! The per-signal summary: bounded, rebuilt, and right after a compaction.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{STREAM, Scratch, chunk, open, sealed, secs};
use norn_core::SignalId;
use norn_index::{IndexOptions, TemporalIndex};
use norn_storage::{SegmentName, StorageOptions, compact};

const SIGNAL: SignalId = SignalId::new(3);

#[test]
fn an_absent_signal_and_a_silent_one_touch_almost_nothing() {
    let s = Scratch::new();
    sealed(&s.dir(), 0, &[chunk(3, 100, 2, 10)]); // the signal, then silence
    for seq in 1..40u64 {
        sealed(&s.dir(), seq, &[chunk(9, 1_000 * seq as i64, 2, 10)]);
    }
    drop(open(&s));
    let index = open(&s);
    let snap = index.snapshot();
    let absent = SignalId::new(77);
    assert_eq!(
        snap.nearest_at_or_before(absent, secs(1 << 20)).unwrap(),
        None
    );
    assert_eq!(index.stats().detail_opens, 0, "absence costs no read");
    let found = snap
        .nearest_at_or_before(SIGNAL, secs(1 << 20))
        .unwrap()
        .unwrap();
    assert_eq!((found.min_ts(), found.max_ts()), (secs(100), secs(110)));
    assert!(index.stats().detail_opens <= 2, "{:?}", index.stats());
    let seen = snap.last_seen(SIGNAL).unwrap();
    assert_eq!((seen.first, seen.last), (secs(100), secs(110)));
    assert_eq!(snap.last_seen(absent), None);
}

#[test]
fn the_summary_survives_a_restart_and_is_rebuilt_when_its_file_is_lost() {
    let s = Scratch::new();
    sealed(&s.dir(), 0, &[chunk(3, 100, 2, 10)]);
    let seen = open(&s).snapshot().last_seen(SIGNAL);
    assert!(seen.is_some());
    let file = s.index_dir().join("signals.nsig");
    assert_eq!(open(&s).snapshot().last_seen(SIGNAL), seen);
    assert_eq!(open(&s).stats().detail_opens, 0, "read from the file");
    std::fs::write(&file, b"garbage").unwrap();
    assert_eq!(open(&s).snapshot().last_seen(SIGNAL), seen);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(open(&s).snapshot().last_seen(SIGNAL), seen);
    sealed(&s.dir(), 1, &[chunk(3, 200, 2, 10)]);
    let later = open(&s).snapshot().last_seen(SIGNAL).unwrap();
    assert_eq!(later.last, secs(210), "a new segment makes the file stale");
}

#[test]
fn the_per_signal_summary_follows_a_compaction_and_a_restart() {
    let s = Scratch::new();
    let (index, _) = TemporalIndex::open(s.root(), IndexOptions::default()).unwrap();
    let mut inputs = Vec::new();
    for seq in 0..4u64 {
        let summary = sealed(&s.dir(), seq, &[chunk(1, seq as i64 * 100, 3, 10)]);
        inputs.push(summary.path.clone());
        index.segment_sealed(STREAM, &summary).unwrap();
    }
    let signal = SignalId::new(1);
    let before = index.snapshot().last_seen(signal).unwrap();
    let opts = StorageOptions::default();
    let outcome = compact(&s.dir(), &inputs, &opts).unwrap();
    index.compaction_applied(STREAM, &outcome).unwrap();
    let after = index.snapshot().last_seen(signal).unwrap();
    assert_eq!((after.first, after.last), (before.first, before.last));
    assert_eq!(
        after.last_segment,
        SegmentName::parse(&outcome.output.path).unwrap()
    );
    let nearest = index.snapshot().nearest_at_or_before(signal, secs(1 << 30));
    assert_eq!(nearest.unwrap().unwrap().max_ts(), after.last);
    drop(index);
    let reopened = open(&s);
    assert_eq!(reopened.snapshot().last_seen(signal), Some(after));
}
