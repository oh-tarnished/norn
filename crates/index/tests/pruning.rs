//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A lookup touches only what it must, and memory stays inside its bounds.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{STREAM, Scratch, chunk, open, plan, sealed, secs};
use norn_core::{Config, SignalId};
use norn_index::{IndexError, IndexOptions, PlanRequest, SignalFilter, TemporalIndex};

/// 50 segments, 100 seconds each, of signals 0..200.
fn hundred(s: &Scratch) {
    for seq in 0..50u64 {
        let chunks: Vec<_> = (0..200)
            .map(|n| chunk(n, seq as i64 * 100, 3, 10))
            .collect();
        sealed(&s.dir(), seq, &chunks);
    }
}

fn reopen(s: &Scratch) -> TemporalIndex {
    drop(open(s));
    let index = open(s);
    assert_eq!(index.stats().segment_opens, 0, "opened from the summary");
    index
}

#[test]
fn a_time_range_never_opens_an_irrelevant_segment_or_sidecar() {
    let s = Scratch::new();
    hundred(&s);
    let index = reopen(&s);
    // Only segment 7 overlaps. Everything else may as well not exist.
    for seq in (0..50).filter(|q| *q != 7) {
        fs::remove_file(s.sidecar(seq, 0)).unwrap();
    }
    let request =
        PlanRequest::new(secs(700), secs(720)).signals(SignalFilter::one(SignalId::new(9)));
    assert_eq!(plan(&index.snapshot(), request).len(), 1);
    let stats = index.stats();
    assert_eq!(stats.segment_opens, 0);
    assert_eq!(stats.segment_bytes_read, 0);
    assert_eq!(stats.detail_repairs, 0);
    assert_eq!(
        stats.detail_opens, 2,
        "a head and the one page by the fence"
    );
}

#[test]
fn one_signal_reads_a_small_part_of_a_sidecar() {
    let s = Scratch::new();
    let chunks: Vec<_> = (0..2_000).map(|n| chunk(n, 0, 3, 10)).collect();
    sealed(&s.dir(), 0, &chunks);
    let index = reopen(&s);
    let whole = fs::metadata(s.sidecar(0, 0)).unwrap().len();
    let request = PlanRequest::everything().signals(SignalFilter::one(SignalId::new(900)));
    assert_eq!(plan(&index.snapshot(), request).len(), 1);
    let read = index.stats().detail_bytes_read;
    assert!(read * 5 < whole, "read {read} of {whole}");
}

#[test]
fn a_signal_outside_a_segments_range_costs_no_read_at_all() {
    let s = Scratch::new();
    hundred(&s);
    let index = reopen(&s);
    let request = PlanRequest::everything().signals(SignalFilter::one(SignalId::new(5_000)));
    assert!(plan(&index.snapshot(), request).is_empty());
    assert_eq!(index.stats().detail_opens, 0);
}

#[test]
fn a_plan_dropped_early_leaves_later_segments_untouched() {
    let s = Scratch::new();
    hundred(&s);
    let index = reopen(&s);
    let request = PlanRequest::everything().signals(SignalFilter::one(SignalId::new(1)));
    let mut p = index.snapshot().plan(request);
    p.next().unwrap().unwrap();
    assert!(index.stats().detail_opens <= 6, "{:?}", index.stats());
}

#[test]
fn a_repeated_lookup_is_served_from_the_cache() {
    let s = Scratch::new();
    hundred(&s);
    let index = reopen(&s);
    let request = PlanRequest::new(secs(700), secs(720));
    plan(&index.snapshot(), request.clone());
    let opens = index.stats().detail_opens;
    plan(&index.snapshot(), request);
    assert_eq!(index.stats().detail_opens, opens);
    assert!(index.stats().cache_hits > 0);
}

#[test]
fn the_cache_never_passes_what_the_budget_leaves() {
    let s = Scratch::new();
    hundred(&s);
    let mut options = IndexOptions::from_config(&Config::default());
    options.memory_budget_bytes = 64 * 1024;
    drop(open(&s));
    let (index, _) = TemporalIndex::open(s.root(), options).unwrap();
    let got = plan(&index.snapshot(), PlanRequest::everything());
    assert_eq!(got.len(), 10_000);
    assert!(index.memory_bytes() <= options.memory_budget_bytes);
}

#[test]
fn lists_past_the_budget_are_refused_with_the_numbers() {
    let s = Scratch::new();
    hundred(&s);
    let mut options = IndexOptions::default();
    options.memory_budget_bytes = 1024;
    let refused = TemporalIndex::open(s.root(), options).map(|_| ());
    assert!(matches!(
        refused,
        Err(IndexError::OverBudget { budget: 1024, .. })
    ));
}

#[test]
fn a_tail_past_the_budget_is_refused_and_changes_nothing() {
    let s = Scratch::new();
    let mut options = IndexOptions::default();
    options.memory_budget_bytes = 4 * 1024;
    let (index, _) = TemporalIndex::open(s.root(), options).unwrap();
    let mut w = common::writer(&s.dir(), 0);
    let metas: Vec<_> = (0..500)
        .map(|i| w.append(&chunk(i, 0, 1, 1)).unwrap())
        .collect();
    let name = norn_storage::SegmentName::of(w.header());
    let refused = index.publish_chunks(STREAM, name, &metas);
    assert!(matches!(refused, Err(IndexError::OverBudget { .. })));
    assert_eq!(index.snapshot().tail_chunks(STREAM), 0);
}

#[test]
fn more_overlap_than_a_plan_may_buffer_is_a_typed_stop() {
    let s = Scratch::new();
    hundred(&s);
    let mut options = IndexOptions::default();
    options.max_plan_buffered = 150;
    let (index, _) = TemporalIndex::open(s.root(), options).unwrap();
    let mut p = index.snapshot().plan(PlanRequest::everything());
    assert!(matches!(
        p.next(),
        Some(Err(IndexError::PlanTooLarge { limit: 150, .. }))
    ));
    assert!(p.next().is_none());
}

#[test]
fn a_reversed_range_is_empty() {
    let s = Scratch::new();
    hundred(&s);
    let got = plan(&open(&s).snapshot(), PlanRequest::new(secs(120), secs(110)));
    assert!(got.is_empty());
}
