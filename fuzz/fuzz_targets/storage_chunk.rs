//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A chunk's body once its CRC has passed: sections, blocks and every column
//! kind. Whatever decodes holds the samples it claims to.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::{chunk_budget, meta_from};
use norn_fuzz::{MAX_INPUT, measure};
use norn_storage::StorageOptions;
use norn_storage::fuzzing::decode_chunk;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((head, body)) = data.split_at_checked(8) else {
        return;
    };
    let meta = meta_from(head, 0, body.len() as u32);
    let (chunk, peak) = measure(|| decode_chunk(&meta, body));
    let limit = chunk_budget(&StorageOptions::default());
    assert!(peak <= limit, "decoding held {peak} bytes, above {limit}");
    if let Ok(chunk) = chunk {
        assert_eq!(chunk.timestamps().len(), chunk.len(), "columns disagree");
        assert_eq!(chunk.qualities().len(), chunk.len(), "columns disagree");
        assert_eq!(chunk.sequences().len(), chunk.len(), "columns disagree");
        assert_eq!(
            chunk.samples().count(),
            chunk.len(),
            "a column cannot be read"
        );
    }
});
