//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The tail log over chunk metas read from an unsealed segment: appended in any
//! batches, it answers exactly what a scan of its metas would, and never
//! accounts for less than it holds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_core::Timestamp;
use norn_fuzz::segment::meta_from;
use norn_fuzz::{MAX_INPUT, split};
use norn_index::fuzzing::tail_log;
use norn_storage::format::frame::ChunkMeta;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let stride = usize::from(selector % 16) + 1;
    let metas: Vec<ChunkMeta> = rest
        .chunks_exact(8)
        .enumerate()
        .map(|(i, head)| meta_from(head, 512 * i as u64, u32::from(head[7])))
        .collect();
    let batches: Vec<Vec<ChunkMeta>> = metas.chunks(stride).map(<[ChunkMeta]>::to_vec).collect();
    let (from, to) = (Timestamp::from_nanos(64), Timestamp::from_nanos(192));
    let (len, seen, range, bytes) = tail_log(&batches, from, to);
    let wanted: Vec<ChunkMeta> = metas
        .iter()
        .filter(|m| m.overlaps(from, to))
        .copied()
        .collect();
    assert_eq!(len, metas.len(), "a meta was lost");
    assert_eq!(
        seen, wanted,
        "a scan of the log differs from a scan of its metas"
    );
    assert_eq!(range.is_some(), !metas.is_empty(), "a log's range");
    assert!(
        bytes as usize >= len * size_of::<ChunkMeta>(),
        "accounted {bytes} bytes"
    );
});
