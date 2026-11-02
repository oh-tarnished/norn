//! The index is derived: every bad file is a reported rebuild, never data loss.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{STREAM, Scratch, brute, chunk, keys, open, plan, sealed};
use norn_index::{DiscardReason, IndexOptions, PlanRequest, SummaryState, TemporalIndex};
use norn_storage::{SegmentName, StorageOptions, compact, stream_dir};

fn three_segments(s: &Scratch) {
    for seq in 0..3u64 {
        let chunks: Vec<_> = (0..5).map(|n| chunk(n, seq as i64 * 100, 10, 1)).collect();
        sealed(&s.dir(), seq, &chunks);
    }
}

fn reopened(s: &Scratch) -> norn_index::StreamReport {
    let (index, report) = TemporalIndex::open(s.root(), IndexOptions::default()).unwrap();
    assert_eq!(
        keys(&plan(&index.snapshot(), PlanRequest::everything())),
        keys(&brute(&s.dir())),
        "answers survive whatever happened to the index"
    );
    report.streams.into_iter().next().unwrap()
}

#[test]
fn a_second_open_reads_no_segment_directory() {
    let s = Scratch::new();
    three_segments(&s);
    assert!(reopened(&s).rebuilt());
    let (index, report) = TemporalIndex::open(s.root(), IndexOptions::default()).unwrap();
    assert!(report.streams[0].clean());
    assert_eq!(report.streams[0].reused, 3);
    assert_eq!(index.stats().segment_opens, 0);
}

#[test]
fn a_rebuild_equals_an_incrementally_maintained_index() {
    let s = Scratch::new();
    let index = open(&s);
    for seq in 0..4u64 {
        let chunks: Vec<_> = (0..5).map(|n| chunk(n, seq as i64 * 100, 10, 1)).collect();
        index
            .segment_sealed(STREAM, &sealed(&s.dir(), seq, &chunks))
            .unwrap();
    }
    let inputs: Vec<_> = (0..2)
        .map(|seq| s.dir().join(SegmentName { seq, generation: 0 }.file_name()))
        .collect();
    let outcome = compact(&s.dir(), &inputs, &StorageOptions::default()).unwrap();
    index.compaction_applied(STREAM, &outcome).unwrap();
    let incremental = index.snapshot();
    let summary = fs::read(s.summary()).unwrap();
    let sidecar = fs::read(s.sidecar(0, 1)).unwrap();

    fs::remove_dir_all(s.root().join("index")).unwrap();
    let rebuilt = open(&s).snapshot();
    assert_eq!(rebuilt.segments(STREAM), incremental.segments(STREAM));
    assert_eq!(
        plan(&rebuilt, PlanRequest::everything()),
        plan(&incremental, PlanRequest::everything())
    );
    assert_eq!(fs::read(s.summary()).unwrap(), summary, "byte for byte");
    assert_eq!(fs::read(s.sidecar(0, 1)).unwrap(), sidecar, "byte for byte");
}

#[test]
fn a_missing_summary_is_rebuilt_and_reported() {
    let s = Scratch::new();
    three_segments(&s);
    let first = reopened(&s);
    assert_eq!(first.summary, SummaryState::Missing);
    assert_eq!(first.read, 3);
}

#[test]
fn every_flipped_byte_of_the_summary_is_rebuilt_and_reported() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    let good = fs::read(s.summary()).unwrap();
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0x40;
        fs::write(s.summary(), &bad).unwrap();
        let r = reopened(&s);
        assert!(
            matches!(r.summary, SummaryState::Discarded(_)),
            "byte {at}: {r:?}"
        );
    }
}

#[test]
fn every_truncation_of_the_summary_is_rebuilt_and_reported() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    let good = fs::read(s.summary()).unwrap();
    for len in (0..good.len()).step_by(7) {
        fs::write(s.summary(), &good[..len]).unwrap();
        assert!(reopened(&s).rebuilt(), "cut at {len}");
    }
}

#[test]
fn a_summary_from_a_newer_norn_is_rebuilt_rather_than_misread() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    let mut bytes = fs::read(s.summary()).unwrap();
    bytes[8] = 9;
    fs::write(s.summary(), bytes).unwrap();
    let want = SummaryState::Discarded(DiscardReason::NewerVersion { found: 9 });
    assert_eq!(reopened(&s).summary, want);
}

#[test]
fn a_summary_copied_from_another_stream_is_refused() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    let good = fs::read(s.summary()).unwrap();
    let mut bytes = good.clone();
    bytes[16] ^= 1;
    let end = bytes.len() - 4;
    let crc = norn_storage::crc32c(&bytes[..end]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
    fs::write(s.summary(), bytes).unwrap();
    let want = SummaryState::Discarded(DiscardReason::WrongStream);
    assert_eq!(reopened(&s).summary, want);
}

#[test]
fn a_stale_summary_catches_up_on_new_and_vanished_segments() {
    let s = Scratch::new();
    three_segments(&s);
    reopened(&s);
    fs::remove_file(
        s.dir().join(
            SegmentName {
                seq: 0,
                generation: 0,
            }
            .file_name(),
        ),
    )
    .unwrap();
    sealed(&s.dir(), 3, &[chunk(1, 300, 10, 1)]);
    let r = reopened(&s);
    assert_eq!(
        (r.summary, r.reused, r.read, r.dropped),
        (SummaryState::Loaded, 2, 1, 1)
    );
    assert_eq!(
        r.orphans_removed, 1,
        "the vanished segment's sidecar goes too"
    );
}

#[test]
fn a_database_with_no_streams_opens_empty() {
    let s = Scratch::new();
    fs::remove_dir_all(stream_dir(s.root(), STREAM)).unwrap();
    let (index, report) = TemporalIndex::open(s.root(), IndexOptions::default()).unwrap();
    assert!(report.streams.is_empty());
    assert!(plan(&index.snapshot(), PlanRequest::everything()).is_empty());
}
