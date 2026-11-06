//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `AppendSamples` as the server takes it: metered off the wire in any chunks,
//! counted and stripped when oversized, decoded by prost inside what was reserved,
//! then through the handler's validation.

#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use norn_fuzz::{budget, limits, measure, runtime, settled, split};
use norn_protocol::telemetry::AppendSamplesRequest;
use norn_server::fuzzing::{read_body, validate};
use prost::Message;

/// gRPC's message prefix.
const PREFIX: usize = 5;

/// `data` as one framed gRPC message, split into two DATA frames at `at`.
fn body(data: &[u8], at: usize) -> Vec<Bytes> {
    let mut wire = vec![0u8];
    wire.extend_from_slice(&(data.len() as u32).to_be_bytes());
    wire.extend_from_slice(data);
    let second = wire.split_off(at.min(wire.len()));
    vec![Bytes::from(wire), Bytes::from(second)]
}

fuzz_target!(|data: &[u8]| {
    let limits = limits();
    if data.len() > limits.max_batch_bytes {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let chunks = match selector & 1 {
        // Exactly what a client sends; or the bytes as they are, prefixes and all.
        0 => body(rest, usize::from(selector)),
        _ => vec![Bytes::copy_from_slice(rest)],
    };
    let budget = budget();
    let delivered = runtime().block_on(read_body(chunks, true, budget.clone(), limits));
    for message in delivered {
        let Ok(bytes) = message.result else {
            continue;
        };
        let Some(payload) = bytes.get(PREFIX..) else {
            continue;
        };
        let (request, peak) = measure(|| AppendSamplesRequest::decode(payload));
        let reserved = message.reserved as usize;
        // Twice: prost grows a vector by doubling, and only then fits the count.
        assert!(
            peak <= 2 * reserved + 4096,
            "decoding held {peak} bytes, {reserved} reserved"
        );
        if let Ok(request) = request {
            if message.oversized.is_some() {
                assert!(request.samples.is_empty(), "a stripped batch kept samples");
            }
            let _ = validate(request, limits);
        }
    }
    settled(&budget);
});
