//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The write-ahead journal's replay over arbitrary segment files, exactly as
//! startup recovery runs it: a whole journal replays or it is kept for inspection.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::{STREAM, chunk_budget, file_name, meta_from};
use norn_fuzz::{MAX_INPUT, measure, scratch, split};
use norn_runtime::ingest::fuzzing::read_journal;
use norn_storage::fuzzing::frame;
use norn_storage::{SegmentHeader, StorageOptions};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let file = if selector & 1 == 0 {
        rest.to_vec()
    } else {
        let mut file = SegmentHeader::new(STREAM, 1).encode().to_vec();
        let (head, body) = rest.split_at_checked(8).unwrap_or((&[], rest));
        let meta = meta_from(head, file.len() as u64, body.len() as u32);
        file.extend_from_slice(&frame(&meta, body));
        file
    };
    let path = scratch(&file_name());
    std::fs::write(&path, &file).expect("a scratch file");
    let opts = StorageOptions::default();
    let (read, peak) = measure(|| {
        let mut samples = Vec::new();
        read_journal(&path, &opts, &mut samples).map(|whole| (whole, samples))
    });
    let limit = 2 * chunk_budget(&opts) + 256 * file.len();
    assert!(peak <= limit, "replaying held {peak} bytes, above {limit}");
    drop(read);
});
