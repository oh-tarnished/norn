//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The checks every column codec target runs, over one column type's API.

use norn_codec::{CodecError, CodecId, MAX_BLOCK_BYTES, MAX_BLOCK_VALUES, peek_header};

use crate::measure;

/// One column type's codec API, and how to compare its values: floats by bit
/// pattern, so NaN payloads and `-0.0` count.
#[derive(Debug, Clone, Copy)]
pub struct Column<T: 'static> {
    /// `decode`.
    pub decode: fn(&[u8], &mut Vec<T>) -> Result<(), CodecError>,
    /// `encode`: the smallest codec.
    pub encode: fn(&[T], &mut Vec<u8>) -> Result<CodecId, CodecError>,
    /// `encode_with`.
    pub encode_with: fn(CodecId, &[T], &mut Vec<u8>) -> Result<(), CodecError>,
    /// The codecs to force on arbitrary values.
    pub codecs: &'static [CodecId],
    /// Whether two value lists are the same, bit for bit.
    pub same: fn(&[T], &[T]) -> bool,
}

/// The most a decode may hold at once: the block's bounds (SPEC §16), with
/// room for one doubling of the output, plus the input's own size again for
/// any copy. A decoder above this is not bounded by its limits.
#[must_use]
pub fn budget<T>(input: usize) -> usize {
    let values = MAX_BLOCK_VALUES * size_of::<T>().max(8);
    2 * values + 2 * MAX_BLOCK_BYTES + 4 * input
}

/// Decodes `block`, which must not panic or exceed [`budget`]; whatever
/// decodes must re-encode under its own codec to the same values. Then each
/// of `values` is encoded under every codec that accepts it and must decode
/// to itself.
///
/// # Panics
/// When the contract is broken: that is the finding.
pub fn check<T: std::fmt::Debug>(c: &Column<T>, block: &[u8], values: &[T]) {
    let (decoded, peak) = measure(|| {
        let mut out = Vec::new();
        (c.decode)(block, &mut out).map(|()| out)
    });
    let limit = budget::<T>(block.len());
    assert!(peak <= limit, "decoding held {peak} bytes, above {limit}");
    if let Ok(decoded) = decoded {
        let header = peek_header(block).expect("a block that decodes has a header");
        assert_eq!(
            header.count,
            decoded.len(),
            "the header's count is the truth"
        );
        round_trip(c, Some(header.codec), &decoded);
        round_trip(c, None, &decoded);
    }
    round_trip(c, None, values);
    for codec in c.codecs {
        let mut out = Vec::new();
        if (c.encode_with)(*codec, values, &mut out).is_ok() {
            same_back(c, &out, values);
        }
    }
}

/// Encodes `values` under `codec` (or the smallest), which must succeed for
/// values that came out of a decoder, and decodes them back.
fn round_trip<T: std::fmt::Debug>(c: &Column<T>, codec: Option<CodecId>, values: &[T]) {
    let mut out = Vec::new();
    let encoded = match codec {
        Some(codec) => (c.encode_with)(codec, values, &mut out),
        None => (c.encode)(values, &mut out).map(|_| ()),
    };
    match encoded {
        Ok(()) => same_back(c, &out, values),
        Err(CodecError::TooLarge { .. }) if values.len() > MAX_BLOCK_VALUES => {}
        Err(e) => panic!("{codec:?} refused {} decoded values: {e}", values.len()),
    }
}

fn same_back<T: std::fmt::Debug>(c: &Column<T>, block: &[u8], values: &[T]) {
    let mut back = Vec::new();
    (c.decode)(block, &mut back).expect("an encoded block decodes");
    assert!((c.same)(&back, values), "{values:?} came back as {back:?}");
}

/// `block`'s bytes behind a current header for `codec`, so a mutation reaches
/// the codec's own parser instead of stopping at the identifier.
#[must_use]
pub fn forced(codec: CodecId, rest: &[u8]) -> Vec<u8> {
    let mut block = vec![codec as u8, norn_codec::CODEC_VERSION];
    block.extend_from_slice(rest);
    block
}

/// The `f64` column, compared by bit pattern.
pub const FLOAT64: Column<f64> = Column {
    decode: norn_codec::float64::decode,
    encode: norn_codec::float64::encode,
    encode_with: norn_codec::float64::encode_with,
    codecs: &[CodecId::Float64Gorilla, CodecId::Float64Alp],
    same: |a, b| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits()),
};

/// The `f32` column, compared by bit pattern.
pub const FLOAT32: Column<f32> = Column {
    decode: norn_codec::float32::decode,
    encode: norn_codec::float32::encode,
    encode_with: norn_codec::float32::encode_with,
    codecs: &[CodecId::Float32Gorilla, CodecId::Float32Alp],
    same: |a, b| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits()),
};

/// Values `T` can be read from: their little-endian bit patterns.
pub trait FromBits: Sized {
    /// Every whole value in `data`.
    fn all(data: &[u8]) -> Vec<Self>;
}

impl FromBits for f64 {
    fn all(data: &[u8]) -> Vec<Self> {
        let words = data.chunks_exact(8).filter_map(|c| c.try_into().ok());
        words
            .map(|c| Self::from_bits(u64::from_le_bytes(c)))
            .collect()
    }
}

impl FromBits for f32 {
    fn all(data: &[u8]) -> Vec<Self> {
        let words = data.chunks_exact(4).filter_map(|c| c.try_into().ok());
        words
            .map(|c| Self::from_bits(u32::from_le_bytes(c)))
            .collect()
    }
}

/// [`check`] for a float column: `data` both as a block behind `codec`'s
/// header and as the bit patterns of values to round-trip.
pub fn floats<T: std::fmt::Debug + FromBits>(c: &Column<T>, data: &[u8], codec: CodecId) {
    check(c, &forced(codec, data), &T::all(data));
}
