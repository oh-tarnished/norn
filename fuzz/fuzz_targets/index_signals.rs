//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `signals.nsig`: any bytes parse or send the caller to rebuild, and what parses
//! encodes back to itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::STREAM;
use norn_fuzz::{MAX_INPUT, measure, repaired, split, u32_at};
use norn_index::fuzzing::{decode_signals, encode_signals};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    // The file's own fingerprint, as a caller whose segments match it would ask.
    let print = u32_at(rest, 12);
    let file = repaired(rest, selector & 1 == 1);
    let (entries, peak) = measure(|| decode_signals(&file, STREAM, print));
    let limit = 4 * file.len() + 4096;
    assert!(peak <= limit, "parsing held {peak} bytes, above {limit}");
    if let Some(entries) = entries {
        let encoded = encode_signals(STREAM, print, entries.clone());
        assert_eq!(decode_signals(&encoded, STREAM, print), Some(entries));
    }
});
