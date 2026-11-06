//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `metadata.log` replay: any bytes are a log, a torn tail or a typed refusal.
//! Half the inputs are records behind good checksums, which reach the record
//! decoder and the registry's state. A record re-encodes to itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::{MAX_INPUT, measure, split};
use norn_metadata::fuzzing::{log_header, reencode, replay, same_record};
use norn_storage::crc32c;

/// Splits `data` into records by 2-byte lengths, behind a current header.
fn framed(mut data: &[u8]) -> Vec<u8> {
    let mut log = log_header();
    while let Some(([a, b], rest)) = data.split_first_chunk::<2>().map(|(h, r)| (*h, r)) {
        let len = usize::from(u16::from_le_bytes([a, b])).min(rest.len());
        let (payload, tail) = rest.split_at(len);
        log.extend_from_slice(&(len as u32).to_le_bytes());
        log.extend_from_slice(&crc32c(payload).to_le_bytes());
        log.extend_from_slice(payload);
        if let Some(encoded) = reencode(payload) {
            assert!(
                same_record(payload, &encoded),
                "a record changed on re-encoding"
            );
        }
        data = tail;
    }
    log
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let log = if selector & 1 == 0 {
        rest.to_vec()
    } else {
        framed(rest)
    };
    let (replayed, peak) = measure(|| replay(&log));
    let limit = 256 * log.len() + (1 << 20);
    assert!(peak <= limit, "replaying held {peak} bytes, above {limit}");
    if let Ok(found) = replayed {
        assert!(found.end <= log.len(), "the good records end past the log");
    }
});
