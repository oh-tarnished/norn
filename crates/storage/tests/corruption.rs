//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A flipped byte anywhere yields a typed report; good chunks stay readable.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{Scratch, chunk, read_all, sealed, unsealed};
use norn_core::Timestamp;
use norn_storage::{
    Action, CorruptionKind, RecoveryEvent, RecoveryMode, SegmentReader, StorageError,
    StorageOptions, recover_segment,
};

fn opts() -> StorageOptions {
    StorageOptions::default()
}

#[test]
fn flipping_any_byte_of_a_sealed_segment_is_reported() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 0, &[chunk(1, 0, 20), chunk(2, 0, 20)]);
    let good = fs::read(&seg.path).unwrap();
    assert!(!good.is_empty());
    for i in 0..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 0x40;
        fs::write(&seg.path, &bad).unwrap();
        let r = match recover_segment(&seg.path, &opts(), RecoveryMode::ReportOnly) {
            Ok(r) => r,
            // A flipped version byte fails the CRC first, so only a checked
            // error may come back here, never success.
            Err(e) => panic!("byte {i}: {e}"),
        };
        assert!(
            r.events.iter().any(RecoveryEvent::is_corruption),
            "byte {i} went unnoticed"
        );
    }
}

#[test]
fn a_corrupt_body_is_a_typed_error_and_its_neighbours_stay_readable() {
    let dir = Scratch::new();
    let seg = sealed(
        dir.path(),
        0,
        &[chunk(1, 0, 20), chunk(2, 0, 20), chunk(3, 0, 20)],
    );
    let middle = SegmentReader::open(&seg.path, opts()).unwrap().chunks()[1];
    let mut bytes = fs::read(&seg.path).unwrap();
    bytes[usize::try_from(middle.offset).unwrap() + 50] ^= 1;
    fs::write(&seg.path, &bytes).unwrap();

    let mut r = SegmentReader::open(&seg.path, opts()).unwrap();
    let chunks = r.chunks().to_vec();
    assert!(r.read_chunk(&chunks[0]).is_ok());
    assert!(r.read_chunk(&chunks[2]).is_ok());
    let e = r.read_chunk(&chunks[1]).unwrap_err();
    let StorageError::Corrupt(c) = e else {
        panic!("{e}")
    };
    assert!(matches!(c.kind, CorruptionKind::BodyChecksum { .. }));
    assert_eq!(c.offset, middle.offset);
    assert_eq!(c.len, middle.frame_len());

    let items: Vec<_> = r.samples(None, Timestamp::MIN, Timestamp::MAX).collect();
    assert_eq!(items.iter().filter(|i| i.is_err()).count(), 1);
    assert_eq!(items.iter().filter(|i| i.is_ok()).count(), 40);
}

#[test]
fn repair_leaves_a_corrupt_sealed_chunk_in_place_and_keeps_reporting_it() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 0, &[chunk(1, 0, 20), chunk(2, 0, 20)]);
    let first = SegmentReader::open(&seg.path, opts()).unwrap().chunks()[0];
    let mut bytes = fs::read(&seg.path).unwrap();
    bytes[usize::try_from(first.offset).unwrap() + 60] ^= 1;
    fs::write(&seg.path, &bytes).unwrap();
    for _ in 0..2 {
        let r = recover_segment(&seg.path, &opts(), RecoveryMode::Repair).unwrap();
        assert!(matches!(
            &r.events[..],
            [RecoveryEvent::Corruption {
                action: Action::LeftInPlace,
                ..
            }]
        ));
        assert_eq!(
            fs::read(&seg.path).unwrap(),
            bytes,
            "bytes are never altered"
        );
    }
}

#[test]
fn a_corrupt_directory_is_rebuilt_from_the_frames() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 0, &[chunk(1, 0, 20), chunk(2, 0, 20)]);
    let good = fs::read(&seg.path).unwrap();
    let mut bad = good.clone();
    let n = bad.len();
    bad[n - 30] ^= 1; // Inside the directory.
    fs::write(&seg.path, &bad).unwrap();
    assert!(matches!(
        SegmentReader::open(&seg.path, opts()),
        Err(StorageError::Corrupt(_))
    ));
    let r = recover_segment(&seg.path, &opts(), RecoveryMode::Repair).unwrap();
    assert!(r.events.iter().any(|e| matches!(
        e,
        RecoveryEvent::Corruption { corruption, .. }
            if corruption.kind == CorruptionKind::Directory
    )));
    assert_eq!(fs::read(&seg.path).unwrap(), good);
}

#[test]
fn damage_in_an_unsealed_segment_does_not_cost_the_chunks_after_it() {
    for (name, at) in [("frame header", 10usize), ("body", 60usize)] {
        let dir = Scratch::new();
        let path = unsealed(
            dir.path(),
            0,
            &[chunk(1, 0, 20), chunk(2, 0, 20), chunk(3, 0, 20)],
        );
        let middle = SegmentReader::open(&path, opts()).unwrap().chunks()[1];
        let mut bytes = fs::read(&path).unwrap();
        let start = usize::try_from(middle.offset).unwrap();
        let span = usize::try_from(middle.frame_len()).unwrap();
        bytes[start + at] ^= 0x10;
        fs::write(&path, &bytes).unwrap();

        let r = recover_segment(&path, &opts(), RecoveryMode::Repair).unwrap();
        assert_eq!(r.summary.as_ref().unwrap().chunks, 2, "{name}");
        let found = r.events.iter().find_map(|e| match e {
            RecoveryEvent::Corruption {
                corruption,
                action: Action::Quarantined { to },
            } => Some((corruption.clone(), to.clone())),
            _ => None,
        });
        let (c, to) = found.unwrap_or_else(|| panic!("{name}: {:?}", r.events));
        assert_eq!(
            (c.offset, c.len),
            (middle.offset, middle.frame_len()),
            "{name}"
        );
        assert_eq!(fs::read(to).unwrap(), &bytes[start..start + span], "{name}");
        let got = read_all(&path);
        assert_eq!(got.len(), 40, "{name}");
        assert!(got.iter().all(|s| s.signal != middle.signal), "{name}");
    }
}

#[test]
fn garbage_where_a_header_should_be_is_quarantined_with_the_file_kept_whole() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 0, &[chunk(1, 0, 20)]);
    let mut bad = fs::read(&seg.path).unwrap();
    bad[0] ^= 1;
    fs::write(&seg.path, &bad).unwrap();
    let r = recover_segment(&seg.path, &opts(), RecoveryMode::Repair).unwrap();
    let [
        RecoveryEvent::SegmentQuarantined {
            corruption,
            to: Some(to),
        },
    ] = &r.events[..]
    else {
        panic!("{:?}", r.events);
    };
    assert_eq!(corruption.kind, CorruptionKind::BadMagic);
    assert_eq!(fs::read(to).unwrap(), bad);
}
