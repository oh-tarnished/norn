//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Partial reads touch only what was asked for, counted in bytes.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, chunk, sealed};
use norn_core::{SignalId, Timestamp};
use norn_storage::format::{HEADER_LEN, TRAILER_LEN, directory::dir_len};
use norn_storage::{SegmentReader, StorageOptions};

#[test]
fn reading_one_chunk_reads_only_the_header_directory_and_that_frame() {
    let dir = Scratch::new();
    let chunks: Vec<_> = (0..100).map(|s| chunk(s, 0, 500)).collect();
    let seg = sealed(dir.path(), 0, &chunks);

    let mut r = SegmentReader::open(&seg.path, StorageOptions::default()).unwrap();
    // Trailer and directory head are each read once, then the directory whole.
    let opened = HEADER_LEN as u64 + TRAILER_LEN as u64 + 8 + dir_len(100);
    assert_eq!(r.bytes_read(), opened, "opening decodes no chunk");

    let meta = *r
        .select(Some(SignalId::new(42)), Timestamp::MIN, Timestamp::MAX)
        .next()
        .unwrap();
    assert_eq!(r.bytes_read(), opened, "selecting reads nothing");
    assert_eq!(r.read_chunk(&meta).unwrap().len(), 500);
    assert_eq!(r.bytes_read(), opened + meta.frame_len());
    let other_frames: u64 = r
        .chunks()
        .iter()
        .filter(|c| c.offset != meta.offset)
        .map(|c| c.frame_len())
        .sum();
    assert!(
        other_frames > 50 * meta.frame_len(),
        "the others went unread"
    );
}

#[test]
fn a_time_range_query_decodes_only_overlapping_chunks_of_the_signal() {
    let dir = Scratch::new();
    let mut chunks = Vec::new();
    for hour in 0..24i64 {
        for signal in 0..10u64 {
            chunks.push(chunk(signal, hour * 3600, 100));
        }
    }
    let seg = sealed(dir.path(), 0, &chunks);
    let mut r = SegmentReader::open(&seg.path, StorageOptions::default()).unwrap();
    let opened = r.bytes_read();
    let (from, to) = (
        Timestamp::from_secs(5 * 3600 + 10).unwrap(),
        Timestamp::from_secs(5 * 3600 + 19).unwrap(),
    );
    let signal = SignalId::new(3);
    let want: Vec<_> = r.select(Some(signal), from, to).copied().collect();
    assert_eq!(want.len(), 1);
    let got: Vec<_> = r
        .samples(Some(signal), from, to)
        .map(Result::unwrap)
        .collect();
    assert_eq!(got.len(), 10);
    assert!(
        got.iter()
            .all(|s| s.signal == signal && s.timestamp >= from && s.timestamp <= to)
    );
    assert_eq!(r.bytes_read(), opened + want[0].frame_len());
}

#[test]
fn a_chunk_file_reads_the_header_and_the_frame_and_never_the_directory() {
    let dir = Scratch::new();
    let chunks: Vec<_> = (0..100).map(|s| chunk(s, 0, 500)).collect();
    let seg = sealed(dir.path(), 0, &chunks);
    let r = SegmentReader::open(&seg.path, StorageOptions::default()).unwrap();
    let meta = *r
        .select(Some(SignalId::new(42)), Timestamp::MIN, Timestamp::MAX)
        .next()
        .unwrap();

    let mut f = norn_storage::ChunkFile::open(&seg.path, StorageOptions::default()).unwrap();
    assert_eq!(f.bytes_read(), HEADER_LEN as u64);
    assert_eq!(f.read_chunk(&meta).unwrap().len(), 500);
    assert_eq!(f.bytes_read(), HEADER_LEN as u64 + meta.frame_len());
    assert_eq!(f.header().first_seq, 0);
}
