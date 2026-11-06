//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Byte-string blocks: any bytes decode or are refused within the block's
//! bounds, and every list of byte strings survives every byte codec.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_codec::{CodecId, bytes};
use norn_fuzz::MAX_INPUT;
use norn_fuzz::column::{Column, check};

const COLUMN: Column<Vec<u8>> = Column {
    decode: bytes::decode,
    encode: bytes::encode::<Vec<u8>>,
    encode_with: bytes::encode_with::<Vec<u8>>,
    codecs: &[CodecId::BytesPlain, CodecId::BytesDict],
    same: |a, b| a == b,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let values: Vec<Vec<u8>> = data.split(|b| *b == 0).map(<[u8]>::to_vec).collect();
    check(&COLUMN, data, &values);
});
