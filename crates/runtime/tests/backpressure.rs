//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Every admission outcome is reachable, and memory stays within the budget.
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

use common::{Dir, HeldGate, config, rig};
use norn_runtime::ingest::{AckLevel, Outcome, Reason, SyncGate, THROTTLE_DELAY};

const MIB: u64 = 1 << 20;

/// A rig whose commit thread sits inside its fsync until released.
fn held(dir: &Dir, ingest: u64) -> (common::Rig, Arc<HeldGate>) {
    let gate = Arc::new(HeldGate::default());
    let as_gate: Arc<dyn SyncGate> = gate.clone();
    let r = rig(&dir.0, config(ingest), Some(as_gate));
    let _ = r.batch(0..1, AckLevel::Durable);
    while gate.fsynced() == 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
    (r, gate)
}

#[test]
fn an_idle_stream_accepts() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    let a = r.batch(0..10, AckLevel::Buffered);
    assert_eq!(a.decision.outcome, Outcome::Accept);
    assert_eq!(a.decision.reason, Reason::CapacityAvailable);
    assert_eq!(a.decision.retry_delay, None);
    r.ingest.close();
}

#[test]
fn a_stalled_commit_thread_turns_accept_into_buffer() {
    let dir = Dir::new();
    let (r, gate) = held(&dir, 64 * MIB);
    let mut outcomes = Vec::new();
    for i in 1..40u64 {
        outcomes.push(r.batch(i..i + 1, AckLevel::Buffered).decision);
    }
    let first = outcomes.iter().position(|d| d.outcome == Outcome::Buffer);
    assert!(first.is_some(), "never BUFFER: {outcomes:?}");
    let d = &outcomes[first.unwrap()];
    assert_eq!(d.reason, Reason::WritePathBehind);
    gate.release();
    r.ingest.close();
    assert_eq!(
        r.stored(&dir.0).len(),
        40,
        "every admitted sample is stored"
    );
}

#[test]
fn past_the_high_water_mark_a_batch_is_admitted_and_the_next_is_refused_until_the_delay() {
    let dir = Dir::new();
    let (r, gate) = held(&dir, MIB);
    let mut next = 1u64;
    let throttled = loop {
        let a = r.batch(next..next + 1000, AckLevel::Buffered);
        next += 1000;
        match a.decision.outcome {
            Outcome::Throttle => break a,
            Outcome::Accept | Outcome::Buffer => {}
            Outcome::Reject => panic!("rejected before throttling: {:?}", a.decision),
        }
    };
    assert_eq!(throttled.decision.reason, Reason::BufferHighWater);
    assert_eq!(throttled.decision.retry_delay, Some(THROTTLE_DELAY));
    assert!(throttled.reached.is_some(), "THROTTLE admits the batch");

    let refused = r.batch(next..next + 1, AckLevel::Buffered);
    assert_eq!(refused.decision.reason, Reason::Throttled);
    assert_eq!(refused.decision.retry_delay, Some(THROTTLE_DELAY));

    r.clock.advance(THROTTLE_DELAY / 2);
    let refused = r.batch(next..next + 1, AckLevel::Buffered);
    assert_eq!(refused.decision.retry_delay, Some(THROTTLE_DELAY / 2));

    r.clock.advance(THROTTLE_DELAY / 2);
    let after = r.batch(next..next + 1, AckLevel::Buffered);
    assert_ne!(after.decision.reason, Reason::Throttled);
    gate.release();
    r.ingest.close();
}

#[test]
fn a_full_stream_rejects_with_a_delay_and_never_passes_its_budget() {
    let dir = Dir::new();
    let (r, gate) = held(&dir, MIB);
    let mut rejected = None;
    for i in 0..1000u64 {
        r.clock.advance(THROTTLE_DELAY);
        let a = r.batch(1 + i * 1000..1 + (i + 1) * 1000, AckLevel::Buffered);
        assert!(r.budget.used() <= MIB, "{} bytes", r.budget.used());
        if a.decision.outcome == Outcome::Reject {
            rejected = Some(a);
            break;
        }
    }
    let a = rejected.expect("never rejected");
    assert_eq!(a.decision.reason, Reason::BufferFull);
    assert!(a.decision.retry_delay.is_some());
    assert_eq!(a.reached, None);
    gate.release();
    r.ingest.close();
}

#[test]
fn an_exhausted_budget_rejects_as_memory() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(MIB), None);
    let first = r.batch(0..1, AckLevel::Buffered);
    assert!(first.reached.is_some());
    let spare = r.budget.available();
    let _hog = r.budget.try_reserve(spare).unwrap();
    let a = r.batch(1..1001, AckLevel::Buffered);
    assert_eq!(a.decision.reason, Reason::MemoryExhausted);
    assert!(a.decision.retry_delay.is_some());
    r.ingest.close();
    assert_eq!(
        r.stored(&dir.0).len(),
        1,
        "the refused batch stored nothing"
    );
}

#[test]
fn a_batch_larger_than_a_stream_may_hold_is_refused_permanently() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 << 10), None);
    let a = r.batch(0..2000, AckLevel::Buffered);
    assert_eq!(a.decision.reason, Reason::BatchTooLarge);
    assert_eq!(a.decision.retry_delay, None);
    r.ingest.close();
    assert!(r.stored(&dir.0).is_empty());
}

#[test]
fn shutting_down_rejects_new_batches_and_finishes_admitted_ones() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    r.batch(0..10, AckLevel::Buffered);
    r.ingest.begin_drain();
    let a = r.batch(10..20, AckLevel::Buffered);
    assert_eq!(a.decision.reason, Reason::ShuttingDown);
    assert!(a.decision.retry_delay.is_some());
    r.ingest.close();
    r.ingest.close();
    assert_eq!(r.stored(&dir.0).len(), 10);
}

#[test]
fn memory_returns_to_the_budget_once_a_generation_is_stored() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    let idle = r.budget.used();
    r.batch(0..1000, AckLevel::Buffered);
    assert!(r.budget.used() > idle);
    r.ingest.close();
    assert!(r.budget.used() <= idle + (8 << 20), "{}", r.budget.used());
}
