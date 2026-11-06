//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An online backup, while batches arrive, and what restoring it gives back.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "common_ingest/mod.rs"]
mod common;
#[path = "backup_common/mod.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use common::{Dir, config, rig};
use norn_database::{Manifest, check_backup, restore};
use norn_runtime::ingest::AckLevel;
use norn_runtime::maintenance::Failure;
use support::{numbers, worker};

const MIB: u64 = 1 << 20;

/// What a restored copy of the rig's database holds, read with a fresh start.
fn restored(root: &Path) -> Vec<u64> {
    let r = rig(root, config(64 * MIB), None);
    assert_eq!(r.ingest.stats(r.stream).corrupt_samples, 0);
    r.ingest.close();
    numbers(&r.stored(root))
}

async fn durable(r: &common::Rig, range: std::ops::Range<u64>) {
    let completion = r.batch(range, AckLevel::Durable).completion.unwrap();
    completion.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_holds_what_was_acknowledged_before_it_and_nothing_after() {
    let (data, out, back) = (Dir::new(), Dir::new(), Dir::new());
    let r = rig(&data.0, config(64 * MIB), None);
    let w = worker(&r);
    durable(&r, 0..100).await;
    let report = w.backup(&out.0.join("b"), None).await.unwrap();
    durable(&r, 100..200).await;
    assert_eq!(report.streams, 1);
    assert_eq!(report.copied_files, report.files);
    drop(w);
    r.ingest.close();
    assert_eq!(numbers(&r.stored(&data.0)), (0..200).collect::<Vec<_>>());
    check_backup(&out.0.join("b")).unwrap();
    let target = back.0.join("restored");
    restore(&out.0.join("b"), &target).unwrap();
    assert_eq!(restored(&target), (0..100).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_during_ingest_is_an_exact_prefix_covering_every_acknowledgement_before_it() {
    let (data, out, back) = (Dir::new(), Dir::new(), Dir::new());
    let r = Arc::new(rig(&data.0, config(64 * MIB), None));
    let w = worker(&r);
    let (acked, admitted, stop) = (
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicBool::new(false)),
    );
    let writer = {
        let (r, acked, admitted, stop) = (
            Arc::clone(&r),
            Arc::clone(&acked),
            Arc::clone(&admitted),
            Arc::clone(&stop),
        );
        std::thread::spawn(move || {
            let mut n = 0;
            while !stop.load(Ordering::Relaxed) {
                admitted.store(n + 10, Ordering::SeqCst);
                let admission = r.batch(n..n + 10, AckLevel::Durable);
                admission
                    .completion
                    .unwrap()
                    .blocking_recv()
                    .unwrap()
                    .unwrap();
                n += 10;
                acked.store(n, Ordering::SeqCst);
            }
        })
    };
    while acked.load(Ordering::SeqCst) < 200 {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    let before = acked.load(Ordering::SeqCst);
    w.backup(&out.0.join("b"), None).await.unwrap();
    let after = admitted.load(Ordering::SeqCst);
    // More must arrive after the cut, or this test would prove nothing.
    while acked.load(Ordering::SeqCst) < after + 100 {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    drop(w);
    r.ingest.close();
    let target = back.0.join("restored");
    restore(&out.0.join("b"), &target).unwrap();
    let got = restored(&target);
    let held = got.len() as u64;
    assert_eq!(
        got,
        (0..held).collect::<Vec<_>>(),
        "not a prefix: a gap or a duplicate"
    );
    assert!(
        held >= before,
        "{before} were acknowledged before the backup; it holds {held}"
    );
    assert!(
        held <= after,
        "{held} held, but only {after} had been admitted by its end"
    );
    assert!(
        numbers(&r.stored(&data.0)).len() as u64 > held,
        "the daemon lost samples"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_incremental_copies_only_new_sealed_segments_and_restores_whole() {
    let (data, out, back) = (Dir::new(), Dir::new(), Dir::new());
    let r = rig(&data.0, config(64 * MIB), None);
    let w = worker(&r);
    durable(&r, 0..100).await;
    let full = w.backup(&out.0.join("full"), None).await.unwrap();
    durable(&r, 100..150).await;
    let first = w
        .backup(&out.0.join("one"), Some(&out.0.join("full")))
        .await
        .unwrap();
    assert_eq!(first.files, full.files + 1, "one new segment");
    assert_eq!(
        first.copied_files, 2,
        "the new segment and the catalogue, nothing older"
    );
    assert!(first.copied_bytes < first.bytes);
    let second = w
        .backup(&out.0.join("two"), Some(&out.0.join("one")))
        .await
        .unwrap();
    assert_eq!(second.copied_files, 1, "nothing new: only the catalogue");
    let manifest = Manifest::read(&out.0.join("one")).unwrap();
    assert_eq!(manifest.parent.as_deref(), Some("../full"));
    assert_eq!(check_backup(&out.0.join("two")).unwrap().chain, 3);
    drop(w);
    r.ingest.close();
    let target = back.0.join("restored");
    restore(&out.0.join("two"), &target).unwrap();
    assert_eq!(restored(&target), (0..150).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_backup_into_a_used_directory_is_refused_and_leaves_it_alone() {
    let (data, out) = (Dir::new(), Dir::new());
    let r = rig(&data.0, config(64 * MIB), None);
    let w = worker(&r);
    durable(&r, 0..10).await;
    w.backup(&out.0.join("b"), None).await.unwrap();
    let before = Manifest::read(&out.0.join("b")).unwrap();
    let again = w.backup(&out.0.join("b"), None).await;
    assert!(matches!(again, Err(Failure::Failed(_))), "{again:?}");
    assert_eq!(Manifest::read(&out.0.join("b")).unwrap(), before);
    drop(w);
    r.ingest.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_of_a_stream_with_no_writer_lists_what_is_on_disk() {
    let (data, out, back) = (Dir::new(), Dir::new(), Dir::new());
    let first = rig(&data.0, config(64 * MIB), None);
    first.batch(0..30, AckLevel::Buffered);
    first.ingest.close();
    drop(first);
    let r = rig(&data.0, config(64 * MIB), None);
    let w = worker(&r);
    w.backup(&out.0.join("b"), None).await.unwrap();
    drop(w);
    r.ingest.close();
    let target = back.0.join("restored");
    restore(&out.0.join("b"), &target).unwrap();
    assert_eq!(restored(&target), (0..30).collect::<Vec<_>>());
}
