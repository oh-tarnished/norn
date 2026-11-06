//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `summary.nidx`, the index's snapshot of a stream: any bytes parse or are
//! discarded with a reason, and what parses encodes back to the same bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::STREAM;
use norn_fuzz::{MAX_INPUT, measure, repaired, split};
use norn_index::fuzzing::{decode_summary, encode_summary};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let file = repaired(rest, selector & 1 == 1);
    let (records, peak) = measure(|| decode_summary(&file, STREAM));
    let limit = 2 * file.len() + 4096;
    assert!(peak <= limit, "parsing held {peak} bytes, above {limit}");
    if let Ok(records) = records {
        let encoded = encode_summary(STREAM, &records);
        assert_eq!(decode_summary(&encoded, STREAM).ok(), Some(records));
    }
});
