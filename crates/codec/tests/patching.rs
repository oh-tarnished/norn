//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Patched frame-of-reference miniblocks: sparse outliers become
//! exceptions instead of widening all 128 values around them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

use norn_codec::{CodecId, integer, timestamp};

#[test]
fn a_sparse_outlier_is_patched_instead_of_widening_its_miniblock() {
    let mut ts: Vec<i64> = (0..1280).map(|i| i * 1_000_000_000).collect();
    ts[300] += 4_000_000;
    let mut b = Vec::new();
    let codec = timestamp::encode(&ts, &mut b).unwrap();
    assert_ne!(codec, CodecId::TimestampRegular);
    // Ten miniblocks of a few bytes each, plus a handful of exceptions.
    assert!(b.len() < 64, "{codec:?}: {} bytes", b.len());
    let mut back = Vec::new();
    timestamp::decode(&b, &mut back).unwrap();
    assert_eq!(back, ts);
}

#[test]
fn outliers_at_both_extremes_of_a_miniblock_round_trip() {
    let mut v: Vec<i64> = (0..128).map(|i| i % 4).collect();
    v[5] = i64::MIN;
    v[77] = i64::MAX;
    v[127] = -9_000_000;
    let mut b = Vec::new();
    integer::encode_with(CodecId::IntegerFor, &v, &mut b).unwrap();
    // Unpatched, the 64-bit range would cost 1,024+ bytes; patched, 2-bit
    // values plus three exceptions (two of them 10-byte varints).
    assert!(b.len() < 80, "{} bytes", b.len());
    let mut back = Vec::new();
    integer::decode(&b, &mut back).unwrap();
    assert_eq!(back, v);
}
