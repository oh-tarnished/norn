//! Property tests: decode(encode(x)) == x for every numeric codec, both the
//! measured choice and each codec forced.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

mod common;

use common::*;
use norn_codec::{CodecError, ColumnType, float32, float64, integer, timestamp};
use proptest::prelude::*;

type Enc = fn(norn_codec::CodecId, &[i64], &mut Vec<u8>) -> norn_codec::Result<()>;
type Dec = fn(&[u8], &mut Vec<i64>) -> norn_codec::Result<()>;

fn every_i64_codec(column: ColumnType, enc: Enc, dec: Dec, values: &[i64]) {
    for codec in codecs(column) {
        let mut block = Vec::new();
        match enc(codec, values, &mut block) {
            Ok(()) => {
                let mut back = Vec::new();
                dec(&block, &mut back).unwrap();
                assert_eq!(back, values, "{codec:?}");
            }
            Err(CodecError::NotApplicable { .. }) => assert!(block.is_empty()),
            Err(e) => panic!("{codec:?}: {e}"),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn timestamps_round_trip_under_every_codec(values in i64_column()) {
        let mut block = Vec::new();
        timestamp::encode(&values, &mut block).unwrap();
        let mut back = Vec::new();
        timestamp::decode(&block, &mut back).unwrap();
        prop_assert_eq!(&back, &values);
        every_i64_codec(ColumnType::Timestamp, timestamp::encode_with, timestamp::decode, &values);
    }

    #[test]
    fn integers_round_trip_under_every_codec(values in i64_column()) {
        let mut block = Vec::new();
        integer::encode(&values, &mut block).unwrap();
        let mut back = Vec::new();
        integer::decode(&block, &mut back).unwrap();
        prop_assert_eq!(&back, &values);
        every_i64_codec(ColumnType::Integer, integer::encode_with, integer::decode, &values);
    }

    #[test]
    fn f64_round_trips_bit_for_bit(values in prop::collection::vec(f64_value(), 0..600)) {
        for codec in [None].into_iter().chain(codecs(ColumnType::Float64).into_iter().map(Some)) {
            let mut block = Vec::new();
            match codec {
                Some(c) => float64::encode_with(c, &values, &mut block).unwrap(),
                None => { float64::encode(&values, &mut block).unwrap(); }
            }
            let mut back = Vec::new();
            float64::decode(&block, &mut back).unwrap();
            prop_assert_eq!(bits64(&back), bits64(&values));
        }
    }

    #[test]
    fn f32_round_trips_bit_for_bit(values in prop::collection::vec(f32_value(), 0..600)) {
        for codec in [None].into_iter().chain(codecs(ColumnType::Float32).into_iter().map(Some)) {
            let mut block = Vec::new();
            match codec {
                Some(c) => float32::encode_with(c, &values, &mut block).unwrap(),
                None => { float32::encode(&values, &mut block).unwrap(); }
            }
            let mut back = Vec::new();
            float32::decode(&block, &mut back).unwrap();
            prop_assert_eq!(bits32(&back), bits32(&values));
        }
    }

    #[test]
    fn a_runs_of_decimals_column_selects_alp_and_round_trips(
        start in -5000i64..5000, steps in prop::collection::vec(-1i64..=1, 64..600),
    ) {
        let values: Vec<f64> = steps.iter().scan(start, |d, s| { *d += s; Some(*d as f64 / 10.0) }).collect();
        let mut block = Vec::new();
        let codec = float64::encode(&values, &mut block).unwrap();
        prop_assert_eq!(codec, norn_codec::CodecId::Float64Alp);
        let mut back = Vec::new();
        float64::decode(&block, &mut back).unwrap();
        prop_assert_eq!(bits64(&back), bits64(&values));
    }
}
