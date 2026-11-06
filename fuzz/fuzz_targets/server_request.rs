//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Every other request off the wire: metered in any chunks, then decoded by
//! prost inside what the meter reserved for it.

#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use norn_fuzz::requests::decode;
use norn_fuzz::{MAX_INPUT, budget, limits, measure, runtime, settled, split};
use norn_server::fuzzing::read_body;

/// gRPC's message prefix.
const PREFIX: usize = 5;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let mut wire = vec![0u8];
    wire.extend_from_slice(&(rest.len() as u32).to_be_bytes());
    wire.extend_from_slice(rest);
    let chunks = vec![Bytes::from(wire)];
    let budget = budget();
    let delivered = runtime().block_on(read_body(chunks, false, budget.clone(), limits()));
    for message in delivered {
        let Ok(bytes) = message.result else {
            continue;
        };
        let payload = bytes.get(PREFIX..).unwrap_or_default();
        let (_, peak) = measure(|| decode(usize::from(selector), payload));
        let reserved = message.reserved as usize;
        assert!(
            peak <= reserved + 4096,
            "decoding held {peak} bytes, {reserved} reserved"
        );
    }
    settled(&budget);
});
