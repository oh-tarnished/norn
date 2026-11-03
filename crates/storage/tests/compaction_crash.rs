//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Compaction survives a crash at each of its steps.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{Scratch, read_all, small_chunks};
use norn_storage::{
    RecoveryEvent, RecoveryMode, StorageOptions, compact, list_segments, live_segments,
    recover_stream,
};

fn opts() -> StorageOptions {
    StorageOptions::default()
}

#[test]
fn a_crash_before_the_commit_leaves_the_inputs_live_and_recovery_removes_the_temp() {
    let dir = Scratch::new();
    let (inputs, all) = small_chunks(dir.path());
    let tmp = dir.path().join("0000000000000000-g0001.nseg.tmp");
    fs::write(&tmp, b"half an output").unwrap();
    assert_eq!(live_segments(dir.path()).unwrap().len(), 2);
    let r = recover_stream(dir.path(), &opts(), RecoveryMode::Repair).unwrap();
    assert!(matches!(
        &r.events[..],
        [RecoveryEvent::UncommittedRemoved { removed: true, .. }]
    ));
    assert!(!tmp.exists());
    let n: usize = inputs.iter().map(|p| read_all(p).len()).sum();
    assert_eq!(n, all.len());
}

#[test]
fn a_crash_after_the_commit_is_finished_by_recovery_with_no_sample_lost_or_doubled() {
    let dir = Scratch::new();
    let (inputs, all) = small_chunks(dir.path());
    let saved: Vec<Vec<u8>> = inputs.iter().map(|p| fs::read(p).unwrap()).collect();
    let done = compact(dir.path(), &inputs, &opts()).unwrap();
    // Rewind to "renamed, nothing deleted", then to "one input deleted".
    for keep in [2usize, 1] {
        for (p, b) in inputs.iter().zip(&saved).take(keep) {
            fs::write(p, b).unwrap();
        }
        let live = live_segments(dir.path()).unwrap();
        assert_eq!(live.len(), 1, "readers see only the output");
        assert_eq!(live[0].path, done.output.path);
        let r = recover_stream(dir.path(), &opts(), RecoveryMode::Repair).unwrap();
        let removed = r
            .events
            .iter()
            .filter(|e| matches!(e, RecoveryEvent::SupersededRemoved { removed: true, .. }));
        assert_eq!(removed.count(), keep);
        assert_eq!(r.segments.len(), 1);
        assert_eq!(list_segments(dir.path()).unwrap().len(), 1);
        assert_eq!(read_all(&done.output.path).len(), all.len());
    }
}

#[test]
fn a_damaged_output_never_causes_its_inputs_to_be_deleted() {
    let dir = Scratch::new();
    let (inputs, _) = small_chunks(dir.path());
    let saved: Vec<Vec<u8>> = inputs.iter().map(|p| fs::read(p).unwrap()).collect();
    let done = compact(dir.path(), &inputs, &opts()).unwrap();
    for (p, b) in inputs.iter().zip(&saved) {
        fs::write(p, b).unwrap();
    }
    let mut out = fs::read(&done.output.path).unwrap();
    out[100] ^= 1;
    fs::write(&done.output.path, &out).unwrap();
    let r = recover_stream(dir.path(), &opts(), RecoveryMode::Repair).unwrap();
    assert!(r.corruptions().count() >= 1);
    assert!(inputs.iter().all(|p| p.exists()));
}
