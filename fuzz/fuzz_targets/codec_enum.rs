//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Enumeration blocks: any bytes decode or are refused within the block's bounds, and every code list survives both dictionary codecs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_codec::CodecId;
use norn_fuzz::MAX_INPUT;
use norn_fuzz::column::{Column, check};

const COLUMN: Column<u32> = Column {
    decode: norn_codec::enumeration::decode,
    encode: norn_codec::enumeration::encode,
    encode_with: norn_codec::enumeration::encode_with,
    codecs: &[CodecId::EnumDict, CodecId::EnumDictRle],
    same: |a, b| a == b,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    // Codes from a small alphabet, as multi-state values are, from each byte.
    let values: Vec<u32> = data.iter().map(|b| u32::from(b % 17)).collect();
    check(&COLUMN, data, &values);
});
