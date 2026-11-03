//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The write path: acknowledgement, atomic batches, duplicates, recovery.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "common_ingest/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Dir, HeldGate, config, rig, sample};
use norn_runtime::ingest::{AckLevel, Batch, Outcome, Reason, SyncGate};

const MIB: u64 = 1 << 20;

#[tokio::test(flavor = "multi_thread")]
async fn durable_is_not_acknowledged_until_the_fsync_has_returned() {
    let dir = Dir::new();
    let gate = Arc::new(HeldGate::default());
    let as_gate: Arc<dyn SyncGate> = gate.clone();
    let r = rig(&dir.0, config(64 * MIB), Some(as_gate));
    let mut admitted = r.batch(0..10, AckLevel::Durable);
    assert_eq!(admitted.reached, Some(AckLevel::Buffered));
    let mut done = admitted.completion.take().unwrap();
    while gate.fsynced() == 0 {
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(done.try_recv().is_err(), "acknowledged before the fsync");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(done.try_recv().is_err(), "acknowledged inside the gate");
    gate.release();
    assert_eq!(done.await.unwrap().unwrap(), AckLevel::Durable);
    assert_eq!(gate.events.lock().unwrap()[..2], ["before", "after"]);
    r.ingest.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn many_batches_share_one_fsync() {
    let dir = Dir::new();
    let gate = Arc::new(HeldGate::default());
    gate.release();
    let as_gate: Arc<dyn SyncGate> = gate.clone();
    let r = rig(&dir.0, config(64 * MIB), Some(as_gate));
    let waits: Vec<_> = (0..100u64)
        .map(|i| {
            r.batch(i * 10..(i + 1) * 10, AckLevel::Durable)
                .completion
                .unwrap()
        })
        .collect();
    for wait in waits {
        assert_eq!(wait.await.unwrap().unwrap(), AckLevel::Durable);
    }
    assert!(gate.fsynced() < 100, "{} fsyncs", gate.fsynced());
    r.ingest.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_fsync_fails_the_waiting_batch_and_refuses_the_next() {
    let dir = Dir::new();
    let gate = Arc::new(HeldGate::default());
    *gate.fail.lock().unwrap() = true;
    let as_gate: Arc<dyn SyncGate> = gate.clone();
    let r = rig(&dir.0, config(64 * MIB), Some(as_gate));
    let wait = r.batch(0..10, AckLevel::Durable).completion.unwrap();
    assert!(wait.await.unwrap().is_err());
    let next = r.batch(10..20, AckLevel::Buffered);
    assert_eq!(next.decision.reason, Reason::StorageUnavailable);
    assert_eq!(next.decision.outcome, Outcome::Reject);
    r.ingest.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn persisted_is_reported_without_any_fsync() {
    let dir = Dir::new();
    let gate = Arc::new(HeldGate::default());
    let as_gate: Arc<dyn SyncGate> = gate.clone();
    let r = rig(&dir.0, config(64 * MIB), Some(as_gate));
    let wait = r.batch(0..10, AckLevel::Persisted).completion.unwrap();
    assert_eq!(wait.await.unwrap().unwrap(), AckLevel::Persisted);
    assert_eq!(gate.entered(), 0);
    gate.release();
    r.ingest.close();
}

#[test]
fn every_admitted_sample_is_stored_once_after_a_clean_close() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    for i in 0..50u64 {
        let a = r.batch(i * 100..(i + 1) * 100, AckLevel::Buffered);
        assert!(a.reached.is_some());
        assert!(a.completion.is_none());
    }
    r.ingest.close();
    let stored = r.stored(&dir.0);
    assert_eq!(stored.len(), 5_000);
    assert_eq!(stored[1234].sequence.get(), 1234);
    assert_eq!(r.ingest.stats(r.stream).samples, 5_000);
}

#[test]
fn a_resend_is_counted_and_not_stored_again_even_across_a_restart() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    assert_eq!(r.batch(0..100, AckLevel::Buffered).duplicates, 0);
    assert_eq!(r.batch(0..100, AckLevel::Buffered).duplicates, 100);
    assert_eq!(r.batch(50..150, AckLevel::Buffered).duplicates, 50);
    r.ingest.close();
    assert_eq!(r.stored(&dir.0).len(), 150);
    drop(r);
    let r = rig(&dir.0, config(64 * MIB), None);
    let a = r.batch(100..200, AckLevel::Buffered);
    assert_eq!(a.duplicates, 50, "the newest chunk rebuilds the window");
    r.ingest.close();
    assert_eq!(r.stored(&dir.0).len(), 200);
}

#[test]
fn a_late_sample_never_seen_is_kept() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    r.batch(100..200, AckLevel::Buffered);
    let late = r.batch(0..10, AckLevel::Buffered);
    assert_eq!(late.duplicates, 0);
    r.ingest.close();
    assert_eq!(r.stored(&dir.0).len(), 110);
}

#[test]
fn a_duplicate_inside_one_batch_is_counted() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    let samples = vec![
        sample(r.signal, 1),
        sample(r.signal, 1),
        sample(r.signal, 2),
    ];
    let a = r
        .ingest
        .admit(Batch::new(r.stream, samples, AckLevel::Buffered));
    assert_eq!(a.duplicates, 1);
    r.ingest.close();
    assert_eq!(r.stored(&dir.0).len(), 2);
}

#[test]
fn durable_is_refused_not_weakened_when_the_daemon_has_no_fsync() {
    let dir = Dir::new();
    let mut c = norn_core::StorageConfig::default();
    c.durable_acks = false;
    let cfg = norn_core::Config::builder().storage(c).build().unwrap();
    let r = rig(&dir.0, cfg, None);
    let a = r.batch(0..10, AckLevel::Durable);
    assert_eq!(a.decision.reason, Reason::AckLevelUnavailable);
    r.ingest.close();
}
