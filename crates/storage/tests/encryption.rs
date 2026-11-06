//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Encrypted segments: the round trip, what reaches the disk, and tampering.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;
mod crypt_common;

use std::fs;

use common::{STREAM, Scratch, bytes_of, chunk};
use crypt_common::{A, all, opts};
use norn_codec::compress::Compression;
use norn_core::{Quality, Sample, Sequence, SignalId, Timestamp, Value, ValueKind};
use norn_storage::format::HEADER_LEN;
use norn_storage::{
    Chunk, SegmentHeader, SegmentReader, SegmentWriter, StorageError, StorageOptions,
};

fn header(seq: u64) -> SegmentHeader {
    SegmentHeader::new(STREAM, seq)
}

fn secret_chunk() -> Chunk {
    let mut c = Chunk::new(SignalId::new(1), ValueKind::Bytes).unwrap();
    let value = Value::Bytes(b"PLAINTEXT-MARKER".to_vec());
    let at = Timestamp::from_nanos(1);
    let sample = Sample::new(at, SignalId::new(1), value, Quality::GOOD, Sequence::ZERO);
    c.push(&sample).unwrap();
    c
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn every_sample_comes_back_and_the_header_says_format_1_1() {
    let s = Scratch::new();
    let o = opts(A);
    let mut w = SegmentWriter::create(s.path(), header(0), o).unwrap();
    let chunks = [chunk(1, 0, 100), chunk(2, 0, 100)];
    let metas: Vec<_> = chunks.iter().map(|c| w.append(c).unwrap()).collect();
    assert_eq!(metas[0].offset, 96, "after the header and the record");
    let path = w.seal().unwrap().path;
    let bytes = fs::read(&path).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 257);
    assert_eq!(bytes[13], 1);
    let mut r = SegmentReader::open(&path, o).unwrap();
    assert!(r.header().encryption.is_some());
    for (meta, chunk) in r.chunks().to_vec().iter().zip(&chunks) {
        let got = r.read_chunk(meta).unwrap();
        assert_eq!(bytes_of(&all(&got)), bytes_of(&all(chunk)));
    }
}

#[test]
fn a_chunk_encoded_ahead_of_time_is_encrypted_when_appended() {
    let s = Scratch::new();
    let o = opts(A);
    let plain = secret_chunk();
    let encoded = plain.encode(&o).unwrap();
    let mut w = SegmentWriter::create(s.path(), header(0), o).unwrap();
    let meta = w.append_encoded(&encoded).unwrap();
    let path = w.seal().unwrap().path;
    assert!(!contains(&fs::read(&path).unwrap(), b"PLAINTEXT-MARKER"));
    let got = SegmentReader::open(&path, o)
        .unwrap()
        .read_chunk(&meta)
        .unwrap();
    assert_eq!(bytes_of(&all(&got)), bytes_of(&all(&plain)));
}

#[test]
fn values_are_on_disk_in_a_plain_segment_and_not_in_an_encrypted_one() {
    let (s, plain) = (Scratch::new(), secret_chunk());
    let mut p = SegmentWriter::create(s.path(), header(0), StorageOptions::default()).unwrap();
    p.append_with(&plain, Compression::None).unwrap();
    let mut e = SegmentWriter::create(s.path(), header(1), opts(A)).unwrap();
    e.append_with(&plain, Compression::None).unwrap();
    let (p, e) = (p.seal().unwrap().path, e.seal().unwrap().path);
    assert!(
        contains(&fs::read(p).unwrap(), b"PLAINTEXT-MARKER"),
        "the test can see it"
    );
    assert!(!contains(&fs::read(e).unwrap(), b"PLAINTEXT-MARKER"));
}

#[test]
fn a_plain_segment_is_still_format_1_0_byte_for_byte() {
    let s = Scratch::new();
    let mut w = SegmentWriter::create(s.path(), header(0), StorageOptions::default()).unwrap();
    let meta = w.append(&chunk(1, 0, 10)).unwrap();
    let path = w.seal().unwrap().path;
    let bytes = fs::read(&path).unwrap();
    assert_eq!(meta.offset, HEADER_LEN as u64);
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 1);
    assert_eq!(bytes[13], 0);
}

#[test]
fn a_format_this_build_does_not_know_is_refused_as_newer() {
    let s = Scratch::new();
    let w = SegmentWriter::create(s.path(), header(0), StorageOptions::default()).unwrap();
    let path = w.seal().unwrap().path;
    for version in [2u16, 0x0201, 0x0100] {
        let mut bytes = fs::read(&path).unwrap();
        bytes[8..10].copy_from_slice(&version.to_le_bytes());
        let crc = norn_storage::crc32c(&bytes[..44]);
        bytes[44..48].copy_from_slice(&crc.to_le_bytes());
        fs::write(&path, bytes).unwrap();
        let got = SegmentReader::open(&path, StorageOptions::default());
        assert!(
            matches!(got, Err(StorageError::UnsupportedVersion { .. })),
            "{version}"
        );
    }
}

#[test]
fn the_same_chunk_never_encrypts_to_the_same_bytes_in_two_segments() {
    let (s, o) = (Scratch::new(), opts(A));
    let body = |seq| {
        let mut w = SegmentWriter::create(s.path(), header(seq), o).unwrap();
        w.append(&chunk(1, 0, 50)).unwrap();
        let bytes = fs::read(w.seal().unwrap().path).unwrap();
        bytes[96 + 48..96 + 80].to_vec()
    };
    assert_ne!(body(0), body(1));
}
