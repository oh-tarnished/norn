//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Edge cases named in the brief: empty and single-element blocks, i64
//! extremes, NaN payloads, signed zeros, infinities; plus codec selection
//! and header behaviour.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

mod common;

use common::*;
use norn_codec::compress::{self, Compression, Stored};
use norn_codec::*;

fn ts_round_trip(values: &[i64]) -> CodecId {
    let mut block = Vec::new();
    let codec = timestamp::encode(values, &mut block).unwrap();
    let mut back = Vec::new();
    timestamp::decode(&block, &mut back).unwrap();
    assert_eq!(back, values);
    codec
}

#[test]
fn empty_blocks_round_trip_for_every_column() {
    ts_round_trip(&[]);
    let mut b = Vec::new();
    float64::encode(&[], &mut b).unwrap();
    let mut f = Vec::new();
    float64::decode(&b, &mut f).unwrap();
    assert!(f.is_empty());
    let (mut b, mut out) = (Vec::new(), Vec::<Vec<u8>>::new());
    bytes::encode::<Vec<u8>>(&[], &mut b).unwrap();
    bytes::decode(&b, &mut out).unwrap();
    assert!(out.is_empty());
    let (mut b, mut out) = (Vec::new(), Vec::new());
    enumeration::encode(&[], &mut b).unwrap();
    enumeration::decode(&b, &mut out).unwrap();
    assert!(out.is_empty());
}

#[test]
fn single_element_blocks_round_trip() {
    ts_round_trip(&[42]);
    ts_round_trip(&[i64::MIN]);
    for v in [f64::NAN, -0.0, 1.5, f64::NEG_INFINITY] {
        let mut b = Vec::new();
        float64::encode(&[v], &mut b).unwrap();
        let mut out = Vec::new();
        float64::decode(&b, &mut out).unwrap();
        assert_eq!(bits64(&out), bits64(&[v]));
    }
}

#[test]
fn i64_extremes_round_trip_under_every_codec() {
    let values = [i64::MIN, i64::MAX, 0, i64::MIN, -1, i64::MAX, i64::MAX];
    for codec in codecs(ColumnType::Integer) {
        let mut b = Vec::new();
        match integer::encode_with(codec, &values, &mut b) {
            Ok(()) => {
                let mut out = Vec::new();
                integer::decode(&b, &mut out).unwrap();
                assert_eq!(out, values, "{codec:?}");
            }
            Err(e) => assert_eq!(e, CodecError::NotApplicable { codec }),
        }
    }
}

#[test]
fn nan_payloads_and_signed_zeros_survive_both_float_codecs() {
    let values = [
        f64::from_bits(0x7ff0_0000_0000_0001),
        f64::from_bits(0xfff8_0000_0000_0042),
        -0.0,
        0.0,
        21.5,
    ];
    for codec in codecs(ColumnType::Float64) {
        let mut b = Vec::new();
        float64::encode_with(codec, &values, &mut b).unwrap();
        let mut out = Vec::new();
        float64::decode(&b, &mut out).unwrap();
        assert_eq!(bits64(&out), bits64(&values), "{codec:?}");
    }
}

#[test]
fn a_regular_series_is_stored_as_start_and_step() {
    let ts: Vec<i64> = (0..86_400)
        .map(|i| 1_700_000_000_000_000_000 + i * 1_000_000_000)
        .collect();
    let mut b = Vec::new();
    assert_eq!(
        timestamp::encode(&ts, &mut b).unwrap(),
        CodecId::TimestampRegular
    );
    assert!(b.len() < 24, "{} bytes", b.len());
}

#[test]
fn forcing_regular_on_an_irregular_series_is_not_applicable() {
    let mut b = vec![9];
    let err = timestamp::encode_with(CodecId::TimestampRegular, &[1, 2, 4], &mut b).unwrap_err();
    assert_eq!(
        err,
        CodecError::NotApplicable {
            codec: CodecId::TimestampRegular
        }
    );
    assert_eq!(b, [9], "a refused encode leaves the buffer as it was");
}

#[test]
fn a_block_from_one_column_is_refused_by_another() {
    let mut b = Vec::new();
    timestamp::encode(&[1, 2, 3], &mut b).unwrap();
    let err = integer::decode(&b, &mut Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        CodecError::WrongColumn {
            offset: 0,
            expected: ColumnType::Integer,
            ..
        }
    ));
}

#[test]
fn an_unknown_codec_version_is_refused_rather_than_misread() {
    let mut b = Vec::new();
    timestamp::encode(&[1, 2, 3], &mut b).unwrap();
    b[1] = CODEC_VERSION + 1;
    let err = timestamp::decode(&b, &mut Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        CodecError::UnsupportedVersion {
            offset: 1,
            version: 2,
            ..
        }
    ));
}

#[test]
fn a_header_reports_codec_version_and_count() {
    let mut b = Vec::new();
    boolean::encode(&[true; 300], &mut b).unwrap();
    let h = peek_header(&b).unwrap();
    assert_eq!(
        (h.codec, h.codec_version, h.count),
        (CodecId::BooleanRle, 1, 300)
    );
}

#[test]
fn an_oversized_block_is_refused_on_encode() {
    let values = vec![0i64; MAX_BLOCK_VALUES + 1];
    let err = integer::encode(&values, &mut Vec::new()).unwrap_err();
    assert!(matches!(err, CodecError::TooLarge { .. }));
}

#[test]
fn a_block_zstd_cannot_shrink_is_stored_raw() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let noise: Vec<u8> = (0..4096).map(|_| rng.next() as u8).collect();
    let mut sealed = Vec::new();
    let stored = compress::seal(&noise, Compression::ZstdSmallest, &mut sealed).unwrap();
    assert_eq!(stored, Stored::Raw);
    assert_eq!(sealed.len(), noise.len() + 4);
}

#[test]
fn rare_toggles_select_boolean_rle_and_steady_enums_select_dictionary_rle() {
    let on_off: Vec<bool> = (0..86_400).map(|i| (i / 7_200) % 2 == 0).collect();
    assert_eq!(
        boolean::encode(&on_off, &mut Vec::new()).unwrap(),
        CodecId::BooleanRle
    );
    let modes: Vec<u32> = (0..86_400).map(|i| (i / 3_600) % 4).collect();
    assert_eq!(
        enumeration::encode(&modes, &mut Vec::new()).unwrap(),
        CodecId::EnumDictRle
    );
}
