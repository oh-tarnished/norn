//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A compaction switch is invisible, except for there being fewer chunks.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use common::{STREAM, Scratch, brute, chunk, open, plan, sealed, secs};
use norn_core::Sample;
use norn_index::{IndexError, PlanRequest, Snapshot, TemporalIndex};
use norn_storage::{ChunkFile, SegmentName, StorageOptions, compact};

fn fill(s: &Scratch, index: &TemporalIndex) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for seq in 0..4u64 {
        let start = seq as i64 * 1000;
        let chunks: Vec<_> = (0..20)
            .map(|i| chunk(i % 4, start + i as i64 * 10, 10, 1))
            .collect();
        let summary = sealed(&s.dir(), seq, &chunks);
        paths.push(summary.path.clone());
        index.segment_sealed(STREAM, &summary).unwrap();
    }
    paths
}

/// Every sample the snapshot's plan reaches, per signal, sorted by time.
fn samples(snapshot: &std::sync::Arc<Snapshot>) -> BTreeMap<u64, Vec<(i64, u64)>> {
    let mut out: BTreeMap<u64, Vec<(i64, u64)>> = BTreeMap::new();
    for l in plan(snapshot, PlanRequest::everything()) {
        let mut f = ChunkFile::open(&snapshot.path_of(&l), StorageOptions::default()).unwrap();
        let chunk = f.read_chunk(&l.chunk).unwrap();
        out.entry(l.chunk.signal.get()).or_default().extend(
            chunk
                .samples()
                .map(|s: Sample| (s.timestamp.as_nanos(), s.sequence.get())),
        );
    }
    out.values_mut().for_each(|v| v.sort_unstable());
    out
}

#[test]
fn a_compaction_changes_the_chunks_and_not_the_samples() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    let before = index.snapshot();
    let samples_before = samples(&before);
    let chunks_before = plan(&before, PlanRequest::everything()).len();

    let outcome = compact(&s.dir(), &paths[..3], &StorageOptions::default()).unwrap();
    index.compaction_applied(STREAM, &outcome).unwrap();

    let after = index.snapshot();
    assert!(after.epoch() > before.epoch());
    assert_eq!(samples(&after), samples_before);
    let got = plan(&after, PlanRequest::everything());
    assert!(got.len() < chunks_before);
    assert_eq!(got, brute(&s.dir()));
    assert_eq!(after.segments(STREAM).len(), 2);
    assert!(!s.sidecar(0, 0).exists(), "the input's sidecar went");
    assert!(s.sidecar(0, 1).exists());
}

#[test]
fn an_old_snapshot_still_answers_as_it_did() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    let old = index.snapshot();
    let before = plan(&old, PlanRequest::everything());
    let outcome = compact(&s.dir(), &paths, &StorageOptions::default()).unwrap();
    index.compaction_applied(STREAM, &outcome).unwrap();
    assert_eq!(
        plan(&old, PlanRequest::everything()),
        before,
        "cached pages"
    );
    assert_eq!(old.segments(STREAM).len(), 4);
}

#[test]
fn a_snapshot_outliving_its_files_says_so_and_refresh_recovers() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    index.clear_cache();
    let old = index.snapshot();
    // Compaction ran; the index has not been told, as in the gap between them.
    compact(&s.dir(), &paths, &StorageOptions::default()).unwrap();
    std::fs::remove_dir_all(s.index_dir()).unwrap();
    let first = old.plan(PlanRequest::everything()).next().unwrap();
    assert!(matches!(first, Err(IndexError::SegmentGone { .. })));

    let report = index.refresh(STREAM).unwrap();
    assert_eq!((report.read, report.dropped), (1, 4));
    assert_eq!(
        plan(&index.snapshot(), PlanRequest::everything()),
        brute(&s.dir())
    );
    let again = index.refresh(STREAM).unwrap();
    assert_eq!(
        (again.read, again.dropped, again.reused),
        (0, 0, 1),
        "idempotent"
    );
}

#[test]
fn a_failed_plan_ends_rather_than_skipping_chunks() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    index.clear_cache();
    let old = index.snapshot();
    compact(&s.dir(), &paths, &StorageOptions::default()).unwrap();
    std::fs::remove_dir_all(s.index_dir()).unwrap();
    let mut plan = old.plan(PlanRequest::everything());
    assert!(plan.next().unwrap().is_err());
    assert!(plan.next().is_none());
}

#[test]
fn a_crash_after_the_commit_and_before_the_cleanup_hides_the_inputs() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    let saved: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    compact(&s.dir(), &paths, &StorageOptions::default()).unwrap();
    for (p, bytes) in paths.iter().zip(saved) {
        std::fs::write(p, bytes).unwrap();
    }
    drop(index);
    let reopened = open(&s).snapshot();
    assert_eq!(reopened.segments(STREAM).len(), 1);
    assert_eq!(reopened.segments(STREAM)[0].generation, 1);
    assert_eq!(plan(&reopened, PlanRequest::everything()), brute(&s.dir()));
}

#[test]
fn applying_a_compaction_twice_changes_nothing() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    let outcome = compact(&s.dir(), &paths, &StorageOptions::default()).unwrap();
    index.compaction_applied(STREAM, &outcome).unwrap();
    let once = plan(&index.snapshot(), PlanRequest::everything());
    index.compaction_applied(STREAM, &outcome).unwrap();
    assert_eq!(plan(&index.snapshot(), PlanRequest::everything()), once);
}

#[test]
fn retention_is_picked_up_by_a_refresh() {
    let s = Scratch::new();
    let index = open(&s);
    let paths = fill(&s, &index);
    std::fs::remove_file(&paths[0]).unwrap();
    assert_eq!(index.refresh(STREAM).unwrap().dropped, 1);
    let got = plan(&index.snapshot(), PlanRequest::new(secs(0), secs(999)));
    assert!(got.is_empty());
    let gone = SegmentName {
        seq: 0,
        generation: 0,
    };
    assert!(
        index
            .snapshot()
            .segments(STREAM)
            .iter()
            .all(|r| r.name() != gone)
    );
}
