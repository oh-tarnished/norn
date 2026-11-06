//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Integer blocks: any bytes decode or are refused within the block's bounds, and every integer list survives every integer codec.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_codec::CodecId;
use norn_fuzz::MAX_INPUT;
use norn_fuzz::column::{Column, check};

const COLUMN: Column<i64> = Column {
    decode: norn_codec::integer::decode,
    encode: norn_codec::integer::encode,
    encode_with: norn_codec::integer::encode_with,
    codecs: &[
        CodecId::IntegerRegular,
        CodecId::IntegerDelta,
        CodecId::IntegerDeltaOfDelta,
        CodecId::IntegerFor,
        CodecId::IntegerRle,
    ],
    same: |a, b| a == b,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let values: Vec<i64> = data
        .chunks_exact(8)
        .filter_map(|c| c.try_into().ok().map(i64::from_le_bytes))
        .collect();
    check(&COLUMN, data, &values);
});
