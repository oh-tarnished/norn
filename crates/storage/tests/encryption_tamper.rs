//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Tampering by someone who can fix every checksum but has no key.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;
mod crypt_common;

use std::fs;

use common::{STREAM, Scratch, chunk};
use crypt_common::{A, forge, opts, recrc};
use norn_storage::{
    CorruptionKind, RecoveryEvent, RecoveryMode, SegmentHeader, SegmentReader, SegmentWriter,
    StorageError, StorageOptions, recover_segment,
};

fn header(seq: u64) -> SegmentHeader {
    SegmentHeader::new(STREAM, seq)
}

/// A sealed segment of two chunks, and their metadata.
fn two_chunks(
    s: &Scratch,
    o: StorageOptions,
) -> (std::path::PathBuf, Vec<norn_storage::ChunkMeta>) {
    let mut w = SegmentWriter::create(s.path(), header(0), o).unwrap();
    let metas = vec![
        w.append(&chunk(1, 0, 50)).unwrap(),
        w.append(&chunk(2, 0, 50)).unwrap(),
    ];
    (w.seal().unwrap().path, metas)
}

#[test]
fn a_changed_byte_with_good_crcs_fails_authentication_and_verify_reports_it() {
    let (s, o) = (Scratch::new(), opts(A));
    let (path, metas) = two_chunks(&s, o);
    forge(&path, &metas[0], |frame| frame[60] ^= 1);
    let mut r = SegmentReader::open(&path, o).unwrap();
    let Err(StorageError::Corrupt(c)) = r.read_chunk(&metas[0]) else {
        panic!("must be refused")
    };
    assert_eq!(c.kind, CorruptionKind::Authentication);
    assert!(
        r.read_chunk(&metas[1]).is_ok(),
        "its neighbour stays readable"
    );
    let before = fs::read(&path).unwrap();
    let report = recover_segment(&path, &o, RecoveryMode::ReportOnly).unwrap();
    let found = report.events.iter().any(|e| {
        matches!(e, RecoveryEvent::Corruption { corruption, .. }
            if corruption.kind == CorruptionKind::Authentication)
    });
    assert!(found, "{:?}", report.events);
    assert_eq!(
        before,
        fs::read(&path).unwrap(),
        "verifying changes nothing"
    );
}

#[test]
fn a_changed_signal_time_or_count_with_good_crcs_is_refused_when_unsealed() {
    for at in [8usize, 16, 32] {
        let (s, o) = (Scratch::new(), opts(A));
        let mut w = SegmentWriter::create(s.path(), header(0), o).unwrap();
        let meta = w.append(&chunk(1, 0, 50)).unwrap();
        w.flush().unwrap();
        let path = w.path().to_path_buf();
        forge(&path, &meta, |frame| frame[at] ^= 1);
        let mut r = SegmentReader::open(&path, o).unwrap();
        let moved = r.chunks()[0];
        assert!(
            matches!(
                r.read_chunk(&moved),
                Err(StorageError::Corrupt(c)) if c.kind == CorruptionKind::Authentication
            ),
            "byte {at}"
        );
    }
}

#[test]
fn two_frames_swapped_with_good_crcs_are_refused() {
    let (s, o) = (Scratch::new(), opts(A));
    let (path, metas) = two_chunks(&s, o);
    assert_eq!(
        metas[0].frame_len(),
        metas[1].frame_len(),
        "the test needs equal sizes"
    );
    let mut bytes = fs::read(&path).unwrap();
    let (a, n) = (metas[0].offset as usize, metas[0].frame_len() as usize);
    let (first, second) = bytes[a..].split_at_mut(n);
    first.swap_with_slice(&mut second[..n]);
    recrc(first);
    fs::write(&path, &bytes).unwrap();
    let mut r = SegmentReader::open(&path, o).unwrap();
    let swapped = norn_storage::ChunkMeta {
        signal: metas[1].signal,
        ..metas[0]
    };
    assert!(matches!(
        r.read_chunk(&swapped),
        Err(StorageError::Corrupt(c)) if c.kind == CorruptionKind::Authentication
    ));
}
