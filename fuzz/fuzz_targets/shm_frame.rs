//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A shared-memory frame's path out of the ring: paid for, copied, counted and
//! decoded, treating the peer as hostile, then through the handler's validation.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::{budget, limits, measure, runtime, settled};
use norn_server::fuzzing::{ring_frame, validate};

fuzz_target!(|data: &[u8]| {
    let limits = limits();
    if data.len() > limits.max_batch_bytes {
        return;
    }
    let budget = budget();
    let handle = runtime().handle();
    let (frame, peak) = measure(|| ring_frame(data, &budget, limits, handle));
    if let Ok(frame) = frame {
        let reserved = frame.reserved as usize;
        // The frame's copy is not part of what `take` reserves for decoding.
        let limit = 2 * reserved + 2 * data.len() + 4096;
        assert!(peak <= limit, "decoding held {peak} bytes, above {limit}");
        if frame.oversized.is_some() {
            assert!(
                frame.request.samples.is_empty(),
                "a stripped batch kept samples"
            );
        }
        let _ = validate(frame.request, limits);
    }
    settled(&budget);
});
