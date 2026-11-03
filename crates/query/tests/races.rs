//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Compaction moves files under a running query; nothing is lost or doubled.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{STREAM, Scratch, all_seen, drain, rig, sample, seen, store, target};
use norn_core::{Sample, Value, ValueKind};
use norn_query::{Filter, Order, Output, QueryLimits, QuerySpec, TimeRange};
use norn_storage::{StorageOptions, compact, live_segments};

const SIGNALS: u64 = 4;

fn spec(order: Order) -> QuerySpec {
    QuerySpec {
        targets: (0..SIGNALS).map(|s| target(s, ValueKind::F64)).collect(),
        range: TimeRange::default(),
        filter: Filter::default(),
        output: Output::Samples(order),
        resume: None,
    }
}

fn segments(n: i64) -> Vec<Vec<Sample>> {
    (0..n)
        .map(|seg| {
            let row = |s| (0..100).map(move |t| sample(s, seg * 1_000 + t, Value::F64(t as f64)));
            (0..SIGNALS).flat_map(row).collect()
        })
        .collect()
}

#[test]
fn a_file_compacted_away_mid_read_costs_a_retry_and_no_sample() {
    for (order, call) in [
        (Order::BySignal, 3),
        (Order::ByTime, 3),
        (Order::ByTime, 12),
    ] {
        let s = Scratch::new();
        store(&s.dir(), &segments(6), 25);
        let r = rig(&s.0, 1 << 30, QueryLimits::default());
        let whole = all_seen(&r.engine, spec(order));
        assert_eq!(whole.len(), 2_400);
        // Reads race: the compaction lands from inside the read, after a snapshot.
        let dir = s.dir();
        let inputs: Vec<_> = live_segments(&dir)
            .unwrap()
            .iter()
            .map(|e| e.path.clone())
            .collect();
        let hook = Box::new(move || {
            compact(&dir, &inputs, &StorageOptions::default()).unwrap();
        });
        let start = r.memory.calls.load(Ordering::SeqCst) + call;
        r.memory.at_call.lock().unwrap().insert(start, hook);
        let mut query = r.engine.open(spec(order)).unwrap();
        let got: Vec<_> = drain(&mut query).unwrap().iter().flat_map(seen).collect();
        assert_eq!(got, whole, "{order:?}");
        assert!(query.stats().races >= 1, "the test did not race");
    }
}

#[test]
fn compaction_in_a_loop_beside_queries_changes_no_answer() {
    let s = Scratch::new();
    store(&s.dir(), &segments(8), 10);
    let r = Arc::new(rig(&s.0, 1 << 30, QueryLimits::default()));
    let whole = all_seen(&r.engine, spec(Order::ByTime));
    let done = Arc::new(AtomicBool::new(false));
    let (dir, rig2, done2) = (s.dir(), Arc::clone(&r), Arc::clone(&done));
    let compactor = std::thread::spawn(move || {
        let opts = StorageOptions::default();
        let mut live = live_segments(&dir).unwrap();
        while live.len() > 1 {
            let inputs: Vec<_> = live.iter().take(2).map(|e| e.path.clone()).collect();
            let outcome = compact(&dir, &inputs, &opts).unwrap();
            rig2.index.compaction_applied(STREAM, &outcome).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(3));
            live = live_segments(&dir).unwrap();
        }
        done2.store(true, Ordering::SeqCst);
    });
    let mut rounds = 0;
    while !done.load(Ordering::SeqCst) || rounds == 0 {
        assert_eq!(all_seen(&r.engine, spec(Order::ByTime)), whole);
        rounds += 1;
    }
    compactor.join().unwrap();
    assert_eq!(all_seen(&r.engine, spec(Order::ByTime)), whole, "and after");
}
