//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A journal batch record: any bytes are a batch or are refused, with no
//! allocation past what the bytes can hold, and a batch encodes back to itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_core::Sample;
use norn_fuzz::{MAX_INPUT, measure};
use norn_runtime::ingest::fuzzing::{decode_record, encode_record};

/// A sample's fixed bytes: its count in a record can be no more than this implies.
const FIXED: usize = 26;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let (batch, peak) = measure(|| decode_record(data));
    let limit = 4 * (data.len() / FIXED + 1) * size_of::<Sample>() + 4 * data.len();
    assert!(peak <= limit, "decoding held {peak} bytes, above {limit}");
    if let Some(batch) = batch {
        let encoded = encode_record(&batch).expect("a decoded batch encodes");
        let back = decode_record(&encoded).expect("an encoded batch decodes");
        let again = encode_record(&back).expect("a decoded batch encodes");
        assert_eq!(encoded, again, "a batch changed on its way through");
    }
});
