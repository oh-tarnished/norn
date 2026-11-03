//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A query stays inside its budget, or stops and says so.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::sync::atomic::Ordering;

use common::{Scratch, all_seen, drain, rig, sample, store, target};
use norn_core::{Sample, Value, ValueKind};
use norn_query::{Filter, Order, Output, QueryError, QueryLimits, QuerySpec, TimeRange};

fn signals(n: u64) -> Vec<norn_query::Target> {
    (0..n).map(|s| target(s, ValueKind::F64)).collect()
}

fn spec(n: u64, output: Output) -> QuerySpec {
    QuerySpec {
        targets: signals(n),
        range: TimeRange::default(),
        filter: Filter::default(),
        output,
        resume: None,
    }
}

fn data(n: u64, each: i64) -> Vec<Sample> {
    let row = |s: u64| (0..each).map(move |t| sample(s, t * 10, Value::F64(t as f64)));
    (0..n).flat_map(row).collect()
}

fn tiny() -> QueryLimits {
    let mut limits = QueryLimits::default();
    limits.page_results = 50;
    limits.slice_bytes = 16 << 10;
    limits
}

#[test]
fn a_tiny_budget_streams_everything_and_stays_within_it() {
    let s = Scratch::new();
    store(&s.dir(), &[data(4, 3_000)], 500);
    let budget = 256 << 10;
    let r = rig(&s.0, budget, tiny());
    for order in [Order::BySignal, Order::ByTime] {
        let got = all_seen(&r.engine, spec(4, Output::Samples(order)));
        assert_eq!(got.len(), 12_000);
    }
    let peak = r.memory.peak.load(Ordering::SeqCst);
    assert!(peak <= budget, "peak {peak}");
    assert!(peak > 1 << 10, "nothing was charged: {peak}");
    assert_eq!(
        r.memory.used.load(Ordering::SeqCst),
        0,
        "all of it came back"
    );
}

#[test]
fn a_budget_too_small_for_one_chunk_is_a_typed_stop_not_an_overrun() {
    let s = Scratch::new();
    store(&s.dir(), &[data(1, 3_000)], 3_000);
    let r = rig(&s.0, 4 << 10, tiny());
    let mut query = r
        .engine
        .open(spec(1, Output::Samples(Order::BySignal)))
        .unwrap();
    assert!(matches!(drain(&mut query), Err(QueryError::Memory(_))));
    assert!(r.memory.peak.load(Ordering::SeqCst) <= 4 << 10);
}

#[test]
fn a_consumer_that_keeps_pages_is_charged_for_them() {
    let s = Scratch::new();
    store(&s.dir(), &[data(1, 2_000)], 500);
    let r = rig(&s.0, 1 << 30, tiny());
    let mut query = r
        .engine
        .open(spec(1, Output::Samples(Order::BySignal)))
        .unwrap();
    let page = query.next_page().unwrap().unwrap();
    let held = page.charge.bytes();
    assert!(held > 0);
    drop(page);
    drop(query);
    assert_eq!(r.memory.used.load(Ordering::SeqCst), 0);
}
