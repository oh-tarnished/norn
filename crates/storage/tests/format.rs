//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The byte layout is pinned: changing it without a version bump fails here.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{Scratch, chunk, sealed};
use norn_codec::CODEC_VERSION;
use norn_storage::format::{DIR_ENTRY_LEN, FRAME_HEADER_LEN, HEADER_LEN, TRAILER_LEN};
use norn_storage::{FORMAT_VERSION, SCHEMA_VERSION, SegmentName, crc32c};

#[test]
fn a_sealed_segment_has_the_documented_bytes_at_the_documented_offsets() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 42, &[chunk(9, 100, 5)]);
    let b = fs::read(&seg.path).unwrap();
    assert_eq!(&b[0..8], b"NORNSEG\0");
    assert_eq!(b[8..10], FORMAT_VERSION.to_le_bytes());
    assert_eq!(b[10..12], SCHEMA_VERSION.to_le_bytes());
    assert_eq!(b[12], CODEC_VERSION);
    assert_eq!(b[14..16], 0u16.to_le_bytes());
    assert_eq!(b[16..24], 7u64.to_le_bytes());
    assert_eq!(b[24..32], 42u64.to_le_bytes());
    assert_eq!(b[32..40], 42u64.to_le_bytes());
    assert_eq!(b[44..48], crc32c(&b[..44]).to_le_bytes());

    let f = &b[HEADER_LEN..];
    assert_eq!(&f[0..4], b"NCHK");
    assert_eq!(f[4], 1);
    assert_eq!(f[5], norn_core::ValueKind::F64.tag());
    assert_eq!(f[8..16], 9u64.to_le_bytes());
    assert_eq!(f[16..24], 100_000_000_000i64.to_le_bytes());
    assert_eq!(f[24..32], 104_000_000_000i64.to_le_bytes());
    assert_eq!(f[32..36], 5u32.to_le_bytes());
    let body_len = u32::from_le_bytes(f[36..40].try_into().unwrap()) as usize;
    let body = &f[FRAME_HEADER_LEN..FRAME_HEADER_LEN + body_len];
    assert_eq!(f[40..44], crc32c(body).to_le_bytes());
    assert_eq!(f[44..48], crc32c(&f[..44]).to_le_bytes());

    let dir_at = HEADER_LEN + FRAME_HEADER_LEN + body_len;
    let d = &b[dir_at..];
    assert_eq!(&d[0..4], b"NDIR");
    assert_eq!(d[4..8], 1u32.to_le_bytes());
    let crc_at = 8 + DIR_ENTRY_LEN;
    assert_eq!(d[crc_at..crc_at + 4], crc32c(&d[..crc_at]).to_le_bytes());
    let t = &d[crc_at + 4..];
    assert_eq!(t.len(), TRAILER_LEN);
    assert_eq!(t[..8], (dir_at as u64).to_le_bytes());
    assert_eq!(&t[8..], b"NORNEND\0");
    assert_eq!(seg.bytes, b.len() as u64);
}

#[test]
fn segment_names_round_trip_and_foreign_names_do_not_parse() {
    let n = SegmentName {
        seq: 42,
        generation: 3,
    };
    assert_eq!(n.file_name(), "000000000000002a-g0003.nseg");
    assert_eq!(
        SegmentName::parse(std::path::Path::new(&n.file_name())),
        Some(n)
    );
    for bad in ["x.nseg", "000000000000002a-g0003.nseg.tmp", "2a-g3.nseg"] {
        assert_eq!(SegmentName::parse(std::path::Path::new(bad)), None, "{bad}");
    }
}
