//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A block of every codec, and envelopes around them.

use norn_codec::compress::{Compression, seal};
use norn_codec::{
    CodecError, CodecId, boolean, bytes, enumeration, float32, float64, integer, timestamp,
};

use crate::Corpus;

type Encode<T> = fn(CodecId, &[T], &mut Vec<u8>) -> Result<(), CodecError>;

fn blocks<T>(c: &Corpus, target: &str, encode: Encode<T>, codecs: &[CodecId], values: &[T]) {
    for codec in codecs {
        let mut block = Vec::new();
        if encode(*codec, values, &mut block).is_ok() {
            c.put(target, &format!("{codec:?}"), &block);
        }
    }
}

/// The floats after the codec header, which those two targets supply themselves.
fn floats(c: &Corpus, target: &str, codec: CodecId, wide: &[f64], narrow: &[f32]) {
    let mut block = Vec::new();
    match codec {
        CodecId::Float32Gorilla | CodecId::Float32Alp => {
            float32::encode_with(codec, narrow, &mut block).expect("every float encodes");
        }
        _ => float64::encode_with(codec, wide, &mut block).expect("every float encodes"),
    }
    c.put(
        target,
        &format!("{codec:?}"),
        block.get(2..).unwrap_or_default(),
    );
}

pub fn write(c: &Corpus) {
    let times: Vec<i64> = (0..512).map(|i| 1_700_000_000_000 + i * 1000).collect();
    let jitter: Vec<i64> = (0..512).map(|i| i * i - (i % 7) * 1000).collect();
    use CodecId as C;
    let ts = [
        C::TimestampRegular,
        C::TimestampDelta,
        C::TimestampDeltaOfDelta,
        C::TimestampFor,
        C::TimestampRle,
    ];
    blocks(c, "codec_timestamp", timestamp::encode_with, &ts, &times);
    let ints = [
        C::IntegerRegular,
        C::IntegerDelta,
        C::IntegerDeltaOfDelta,
        C::IntegerFor,
        C::IntegerRle,
    ];
    blocks(c, "codec_integer", integer::encode_with, &ints, &jitter);
    let flags: Vec<bool> = (0..512).map(|i| i % 11 < 3).collect();
    blocks(
        c,
        "codec_boolean",
        boolean::encode_with,
        &[C::BooleanBits, C::BooleanRle],
        &flags,
    );
    let codes: Vec<u32> = (0..512).map(|i| (i / 9) % 5).collect();
    blocks(
        c,
        "codec_enum",
        enumeration::encode_with,
        &[C::EnumDict, C::EnumDictRle],
        &codes,
    );
    let strings: Vec<Vec<u8>> = (0..64)
        .map(|i| format!("s-{}", i % 7).into_bytes())
        .collect();
    let plain = [C::BytesPlain, C::BytesDict];
    blocks(
        c,
        "codec_bytes",
        bytes::encode_with::<Vec<u8>>,
        &plain,
        &strings,
    );

    let wide: Vec<f64> = (0..512)
        .map(|i| match i % 97 {
            0 => f64::NAN,
            1 => -0.0,
            _ => 20.0 + f64::from(i) / 4.0,
        })
        .collect();
    let narrow: Vec<f32> = wide.iter().map(|v| *v as f32).collect();
    floats(c, "codec_gorilla", C::Float64Gorilla, &wide, &narrow);
    floats(c, "codec_gorilla", C::Float32Gorilla, &wide, &narrow);
    floats(c, "codec_alp", C::Float64Alp, &wide, &narrow);
    floats(c, "codec_alp", C::Float32Alp, &wide, &narrow);

    let mut block = Vec::new();
    timestamp::encode(&times, &mut block).expect("timestamps encode");
    let hows = [
        ("none", Compression::None),
        ("default", Compression::Default),
    ];
    for (name, how) in hows {
        let mut sealed = Vec::new();
        seal(&block, how, &mut sealed).expect("a small block seals");
        c.put("codec_zstd", name, &sealed);
    }
    c.selected("codec_zstd", "text", 1, &[b'n'; 4096]);
}
