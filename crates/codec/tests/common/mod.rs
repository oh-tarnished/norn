//! Shared generators and helpers for the codec tests.
#![allow(dead_code, clippy::indexing_slicing)]

use norn_codec::{ALL_CODECS, CodecId, ColumnType};
use proptest::prelude::*;

/// Every codec id belonging to `column`.
pub fn codecs(column: ColumnType) -> Vec<CodecId> {
    ALL_CODECS
        .iter()
        .copied()
        .filter(|c| c.column() == column)
        .collect()
}

/// `i64` values biased towards extremes, small numbers and repeats.
pub fn i64_value() -> impl Strategy<Value = i64> {
    prop_oneof![
        any::<i64>(),
        Just(i64::MIN),
        Just(i64::MAX),
        Just(0),
        -3i64..3,
        1_700_000_000_000_000_000i64..1_700_000_000_100_000_000,
    ]
}

/// Columns of up to 600 values (crossing several 128-value miniblocks),
/// including sorted and run-heavy shapes.
pub fn i64_column() -> impl Strategy<Value = Vec<i64>> {
    prop_oneof![
        prop::collection::vec(i64_value(), 0..600),
        prop::collection::vec(i64_value(), 0..600).prop_map(|mut v| {
            v.sort_unstable();
            v
        }),
        (i64_value(), i64_value(), 0usize..600).prop_map(|(start, step, n)| {
            (0..n as i64)
                .map(|i| start.wrapping_add(step.wrapping_mul(i)))
                .collect()
        }),
        prop::collection::vec((i64_value(), 1usize..40), 0..30).prop_map(|runs| runs
            .into_iter()
            .flat_map(|(v, n)| std::iter::repeat_n(v, n))
            .collect()),
    ]
}

/// `f64` covering every bit pattern (NaN payloads, signed zeros,
/// infinities, subnormals) and decimal-like readings that select ALP.
pub fn f64_value() -> impl Strategy<Value = f64> {
    prop_oneof![
        any::<u64>().prop_map(f64::from_bits),
        (-1_000_000i64..1_000_000).prop_map(|i| i as f64 / 10.0),
        (-4000i64..4000).prop_map(|i| i as f64 * 0.25),
        Just(0.0),
        Just(-0.0),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(f64::from_bits(0x7ff8_dead_beef_0001)),
        Just(f64::MIN_POSITIVE / 3.0),
    ]
}

/// `f32` equivalents of [`f64_value`].
pub fn f32_value() -> impl Strategy<Value = f32> {
    prop_oneof![
        any::<u32>().prop_map(f32::from_bits),
        (-100_000i32..100_000).prop_map(|i| i as f32 / 10.0),
        Just(-0.0f32),
        Just(f32::NAN),
        Just(f32::from_bits(0xffc0_1234)),
        Just(f32::INFINITY),
    ]
}

/// Bit patterns, so NaN compares equal to itself.
pub fn bits64(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// Bit patterns of `f32`.
pub fn bits32(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// A deterministic xorshift generator for mutation tests.
pub struct Rng(pub u64);

impl Rng {
    /// The next pseudo-random value.
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A value in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
