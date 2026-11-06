//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The storage reader over arbitrary files: segment header, chunk frames,
//! directory and checksums. Half the inputs are raw files; the rest are valid
//! frames around arbitrary bodies, which reach what a frame's CRC guards. Recovery
//! then reads the same file, and what it repairs must read back.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::{STREAM, file_name, meta_from, read_all};
use norn_fuzz::{MAX_INPUT, scratch, split};
use norn_storage::fuzzing::frame;
use norn_storage::{RecoveryMode, SegmentHeader, StorageOptions, recover_segment};

/// A valid header, then one valid frame per 8-byte head and 2-byte body length.
fn framed(mut data: &[u8]) -> Vec<u8> {
    let mut file = SegmentHeader::new(STREAM, 1).encode().to_vec();
    while let Some((head, rest)) = data.split_at_checked(10) {
        let len = usize::from(u16::from_le_bytes([head[8], head[9]])).min(rest.len());
        let (body, tail) = rest.split_at(len);
        let meta = meta_from(head, file.len() as u64, len as u32);
        file.extend_from_slice(&frame(&meta, body));
        data = tail;
    }
    file
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let dir = scratch("segment");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(file_name());
    let file = if selector & 1 == 0 {
        rest.to_vec()
    } else {
        framed(rest)
    };
    std::fs::write(&path, &file).expect("a scratch file");
    let opts = StorageOptions::default();
    read_all(&path, opts);
    // Report-only must leave the file alone; repair only runs on some inputs,
    // because it syncs, which would otherwise be most of the run.
    let _ = recover_segment(&path, &opts, RecoveryMode::ReportOnly);
    assert_eq!(
        std::fs::read(&path).ok().as_deref(),
        Some(&file[..]),
        "report-only wrote"
    );
    if selector & 0xf0 == 0xf0 {
        let repaired = recover_segment(&path, &opts, RecoveryMode::Repair);
        if repaired.is_ok() && path.exists() {
            read_all(&path, opts);
        }
    }
});
