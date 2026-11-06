//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The compression envelope and the zstd frames inside it: anything opens or is
//! refused within `MAX_BLOCK_BYTES`, however large a frame claims to expand, and
//! whatever is sealed opens to itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_codec::compress::{Compression, ZstdLevel, open, seal};
use norn_codec::{MAX_BLOCK_BYTES, peek_header};
use norn_fuzz::{MAX_INPUT, measure, split};

/// The output, one decoder window, its buffers, and the input again.
const BUDGET: usize = 4 * MAX_BLOCK_BYTES;

fn opens(envelope: &[u8]) {
    let (opened, peak) = measure(|| {
        let mut out = Vec::new();
        open(envelope, &mut out).map(|_| out)
    });
    assert!(
        peak <= BUDGET + 2 * envelope.len(),
        "opening held {peak} bytes"
    );
    if let Ok(out) = opened {
        assert!(out.len() <= MAX_BLOCK_BYTES, "opened {} bytes", out.len());
        let _ = peek_header(&out);
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    opens(data);
    // A frame behind a valid envelope: the selector's claim of its length.
    let claim = u64::from(selector) << (selector & 0x1f);
    let mut envelope = vec![1u8, 3];
    let mut left = claim;
    while left >= 0x80 {
        envelope.push((left as u8 & 0x7f) | 0x80);
        left >>= 7;
    }
    envelope.push(left as u8);
    envelope.extend_from_slice(rest);
    opens(&envelope);
    let how = match selector % 7 {
        0 => Compression::None,
        1 => Compression::Default,
        n @ 2..=5 => ZstdLevel::new(n + 1).map_or(Compression::Default, Compression::Zstd),
        _ => Compression::ZstdSmallest,
    };
    let mut sealed = Vec::new();
    seal(rest, how, &mut sealed).expect("under the block limit");
    let mut back = Vec::new();
    open(&sealed, &mut back).expect("a sealed block opens");
    assert_eq!(back, rest, "sealing lost bytes");
});
