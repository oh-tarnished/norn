//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! ALP floats: whatever follows an ALP header decodes or is refused
//! within the block's bounds, and every bit pattern survives, NaN payloads too.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_codec::CodecId;
use norn_fuzz::MAX_INPUT;
use norn_fuzz::column::{FLOAT32, FLOAT64, floats, forced};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    floats(&FLOAT64, data, CodecId::Float64Alp);
    floats(&FLOAT32, data, CodecId::Float32Alp);
    let _ = forced(CodecId::Float64Alp, data);
});
