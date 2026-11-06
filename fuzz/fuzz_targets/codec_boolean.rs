//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Boolean blocks: any bytes decode or are refused within the block's bounds, and every boolean list survives both boolean codecs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_codec::CodecId;
use norn_fuzz::MAX_INPUT;
use norn_fuzz::column::{Column, check};

const COLUMN: Column<bool> = Column {
    decode: norn_codec::boolean::decode,
    encode: norn_codec::boolean::encode,
    encode_with: norn_codec::boolean::encode_with,
    codecs: &[CodecId::BooleanBits, CodecId::BooleanRle],
    same: |a, b| a == b,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    // Each byte is a run: its low bit the value, the rest its length.
    let values: Vec<bool> = data
        .iter()
        .flat_map(|b| std::iter::repeat_n(b & 1 == 1, usize::from(b >> 1) + 1))
        .collect();
    check(&COLUMN, data, &values);
});
