//! The clean marker vouches for exactly what it lists, and a bad one for nothing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;

use common::Scratch;
use norn_core::{StreamId, Timestamp};
use norn_storage::{CLEAN_FILE, CleanMarker, CleanSegment, SegmentName};

const STREAM: StreamId = StreamId::new(7);

fn entry(seq: u64, bytes: u64) -> CleanSegment {
    let at = Timestamp::from_nanos(seq as i64);
    CleanSegment {
        name: SegmentName { seq, generation: 0 },
        bytes,
        samples: 10,
        time_range: Some((at, at)),
    }
}

fn file(dir: &std::path::Path, seq: u64, bytes: usize) {
    let name = SegmentName { seq, generation: 0 }.file_name();
    fs::write(dir.join(name), vec![0u8; bytes]).unwrap();
}

#[test]
fn a_marker_reads_back_as_it_was_written() {
    let dir = Scratch::new();
    let marker = CleanMarker::new(STREAM, [entry(0, 100), entry(1, 200)]);
    marker.save(dir.path()).unwrap();
    assert_eq!(CleanMarker::load(dir.path(), STREAM), Some(marker));
}

#[test]
fn a_flipped_byte_anywhere_makes_a_marker_vouch_for_nothing() {
    let dir = Scratch::new();
    CleanMarker::new(STREAM, [entry(0, 100)])
        .save(dir.path())
        .unwrap();
    let path = dir.path().join(CLEAN_FILE);
    let good = fs::read(&path).unwrap();
    for at in 0..good.len() {
        let mut bad = good.clone();
        *bad.get_mut(at).unwrap() ^= 0x40;
        fs::write(&path, &bad).unwrap();
        assert_eq!(CleanMarker::load(dir.path(), STREAM), None, "byte {at}");
    }
}

#[test]
fn a_marker_of_another_stream_vouches_for_nothing() {
    let dir = Scratch::new();
    CleanMarker::new(STREAM, [entry(0, 100)])
        .save(dir.path())
        .unwrap();
    assert_eq!(CleanMarker::load(dir.path(), StreamId::new(8)), None);
}

#[test]
fn only_the_same_name_at_the_same_length_is_vouched_for() {
    let marker = CleanMarker::new(STREAM, [entry(0, 100)]);
    assert!(marker.vouches(entry(0, 0).name, 100));
    assert!(!marker.vouches(entry(0, 0).name, 101));
    assert!(!marker.vouches(entry(1, 0).name, 100));
}

#[test]
fn a_directory_is_current_only_when_it_holds_exactly_what_is_listed() {
    let dir = Scratch::new();
    let journal = dir.path().join("journal");
    let marker = CleanMarker::new(STREAM, [entry(0, 100)]);
    assert!(
        !marker.is_current(dir.path(), &journal).unwrap(),
        "a file is missing"
    );
    file(dir.path(), 0, 100);
    assert!(marker.is_current(dir.path(), &journal).unwrap());
    file(dir.path(), 1, 50);
    assert!(
        !marker.is_current(dir.path(), &journal).unwrap(),
        "a new segment"
    );
    fs::remove_file(dir.path().join(entry(1, 0).name.file_name())).unwrap();
    let tmp = format!("{}.tmp", entry(1, 0).name.file_name());
    fs::write(dir.path().join(&tmp), b"x").unwrap();
    assert!(
        !marker.is_current(dir.path(), &journal).unwrap(),
        "a compaction in flight"
    );
    fs::remove_file(dir.path().join(&tmp)).unwrap();
    fs::create_dir(&journal).unwrap();
    file(&journal, 0, 10);
    assert!(
        !marker.is_current(dir.path(), &journal).unwrap(),
        "a journal segment"
    );
}
