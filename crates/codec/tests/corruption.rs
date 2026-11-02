//! Fuzz-style tests: every truncation and many random corruptions of valid
//! blocks must return a typed error or decode to *something*, never panic,
//! and never allocate beyond the declared limits.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

mod common;

use common::Rng;
use norn_codec::compress::{self, Compression};
use norn_codec::*;

/// One valid block per codec, built from shapes that exercise every path.
fn corpus() -> Vec<(CodecId, Vec<u8>)> {
    // Sparse outliers every 37 samples force patched miniblocks.
    let ts: Vec<i64> = (0..300)
        .map(|i| 1_000 * i + if i % 37 == 0 { 5_000_000 } else { 0 })
        .collect();
    let ints: Vec<i64> = (0..300)
        .map(|i| [i64::MIN, 5, i64::MAX, i * i][i as usize % 4])
        .collect();
    let floats: Vec<f64> = (0..300)
        .map(|i| {
            if i % 50 == 0 {
                f64::NAN
            } else {
                i as f64 / 10.0
            }
        })
        .collect();
    let f32s: Vec<f32> = floats.iter().map(|v| *v as f32).collect();
    let bools: Vec<bool> = (0..300).map(|i| i % 90 < 40).collect();
    let enums: Vec<u32> = (0..300).map(|i| (i / 40) % 5).collect();
    let strings: Vec<Vec<u8>> = (0..300)
        .map(|i| format!("state-{}", i % 6).into_bytes())
        .collect();
    let mut out = Vec::new();
    for codec in ALL_CODECS {
        let mut b = Vec::new();
        let r = match codec.column() {
            ColumnType::Timestamp => timestamp::encode_with(codec, &ts, &mut b),
            ColumnType::Integer => integer::encode_with(codec, &ints, &mut b),
            ColumnType::Float64 => float64::encode_with(codec, &floats, &mut b),
            ColumnType::Float32 => float32::encode_with(codec, &f32s, &mut b),
            ColumnType::Boolean => boolean::encode_with(codec, &bools, &mut b),
            ColumnType::Enum => enumeration::encode_with(codec, &enums, &mut b),
            ColumnType::Bytes => bytes::encode_with(codec, &strings, &mut b),
        };
        if r.is_err() {
            let regular: Vec<i64> = (0..300).map(|i| 7 * i).collect();
            b.clear();
            match codec.column() {
                ColumnType::Timestamp => timestamp::encode_with(codec, &regular, &mut b).unwrap(),
                _ => integer::encode_with(codec, &regular, &mut b).unwrap(),
            }
        }
        out.push((codec, b));
    }
    out
}

/// Decodes `block` as its column; returns the decoded length or the error.
fn decode_any(column: ColumnType, block: &[u8]) -> Result<usize> {
    match column {
        ColumnType::Timestamp => decode_len(block, timestamp::decode),
        ColumnType::Integer => decode_len(block, integer::decode),
        ColumnType::Float64 => decode_len(block, float64::decode),
        ColumnType::Float32 => decode_len(block, float32::decode),
        ColumnType::Boolean => decode_len(block, boolean::decode),
        ColumnType::Enum => decode_len(block, enumeration::decode),
        ColumnType::Bytes => decode_len(block, bytes::decode),
    }
}

fn decode_len<T>(block: &[u8], f: fn(&[u8], &mut Vec<T>) -> Result<()>) -> Result<usize> {
    let mut out = Vec::new();
    f(block, &mut out)?;
    assert!(out.len() <= MAX_BLOCK_VALUES);
    Ok(out.len())
}

#[test]
fn the_corpus_covers_every_codec_and_decodes_cleanly() {
    let corpus = corpus();
    assert_eq!(corpus.len(), ALL_CODECS.len());
    for (codec, block) in &corpus {
        assert_eq!(decode_any(codec.column(), block).unwrap(), 300, "{codec:?}");
    }
}

#[test]
fn every_truncation_of_every_block_is_a_typed_error() {
    for (codec, block) in corpus() {
        for cut in 0..block.len() {
            let err = decode_any(codec.column(), &block[..cut]);
            assert!(err.is_err(), "{codec:?} cut at {cut} decoded");
        }
    }
}

#[test]
fn trailing_bytes_after_a_block_are_reported_with_their_offset() {
    for (codec, mut block) in corpus() {
        let end = block.len();
        block.push(0);
        assert_eq!(
            decode_any(codec.column(), &block),
            Err(CodecError::TrailingBytes { offset: end })
        );
    }
}

#[test]
fn random_corruption_never_panics() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let corpus = corpus();
    for _ in 0..40_000 {
        let (codec, block) = &corpus[rng.below(corpus.len())];
        let mut bad = block.clone();
        for _ in 0..1 + rng.below(4) {
            let i = rng.below(bad.len());
            match rng.below(3) {
                0 => bad[i] ^= 1 << rng.below(8),
                1 => bad[i] = rng.next() as u8,
                _ => bad[i] = 0xff,
            }
        }
        let _ = decode_any(codec.column(), &bad);
    }
}

#[test]
fn a_hostile_count_is_refused_before_allocation() {
    // Header claiming 2^40 values: codec id, version, varint count.
    let block = [
        CodecId::IntegerFor as u8,
        1,
        0x80,
        0x80,
        0x80,
        0x80,
        0x80,
        0x20,
    ];
    let err = integer::decode(&block, &mut Vec::new()).unwrap_err();
    assert!(matches!(err, CodecError::TooLarge { offset: 2, .. }));
}

#[test]
fn corrupted_and_truncated_envelopes_are_typed_errors() {
    let (_, block) = corpus().swap_remove(0);
    let block: Vec<u8> = block.iter().copied().cycle().take(20_000).collect();
    let mut sealed = Vec::new();
    compress::seal(&block, Compression::Default, &mut sealed).unwrap();
    for cut in 0..sealed.len() {
        assert!(
            compress::open(&sealed[..cut], &mut Vec::new()).is_err(),
            "cut {cut}"
        );
    }
    let mut rng = Rng(77);
    for _ in 0..5_000 {
        let mut bad = sealed.clone();
        let i = rng.below(bad.len());
        bad[i] ^= 1 << rng.below(8);
        let mut out = Vec::new();
        if compress::open(&bad, &mut out).is_ok() {
            assert!(out.len() <= MAX_BLOCK_BYTES);
        }
    }
}
