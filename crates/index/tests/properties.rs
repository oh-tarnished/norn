//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Index answers match a brute-force scan over random segment sets.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{STREAM, Scratch, brute, chunk, keys, open, plan, sealed, secs};
use norn_core::SignalId;
use norn_index::{ChunkLocator, PlanRequest, SignalFilter};
use norn_storage::{StorageOptions, compact, live_segments};
use proptest::prelude::*;

/// (signal, start second, samples, step).
type Spec = (u64, i64, usize, i64);

fn specs() -> impl Strategy<Value = Vec<Vec<Spec>>> {
    let chunk = (0u64..12, 0i64..2_000, 1usize..8, 1i64..40);
    prop::collection::vec(prop::collection::vec(chunk, 0..40), 1..7)
}

fn nearest_brute(all: &[ChunkLocator], signal: u64, t: i64) -> Option<ChunkLocator> {
    let t = secs(t);
    all.iter()
        .filter(|c| c.chunk.signal.get() == signal && c.min_ts() <= t)
        .copied()
        .max_by_key(|c| (c.max_ts().min(t), c.min_ts(), *c))
}

fn check_answers(s: &Scratch, from: i64, span: i64, signals: &[u64]) {
    let all = brute(&s.dir());
    let index = open(s);
    let snap = index.snapshot();
    let every = plan(&snap, PlanRequest::everything().stream(STREAM));
    assert_eq!(every, all, "stream -> chunks");

    let (a, b) = (secs(from), secs(from + span));
    let ranged: Vec<_> = all
        .iter()
        .filter(|c| c.chunk.overlaps(a, b))
        .copied()
        .collect();
    assert_eq!(
        plan(&snap, PlanRequest::new(a, b)),
        ranged,
        "time -> chunks"
    );

    let set = SignalFilter::many(signals.iter().copied().map(SignalId::new));
    let want: Vec<_> = ranged
        .iter()
        .filter(|c| set.matches(c.chunk.signal))
        .copied()
        .collect();
    assert_eq!(
        plan(&snap, PlanRequest::new(a, b).signals(set)),
        want,
        "signals -> chunks"
    );

    for signal in signals {
        let got = snap
            .nearest_at_or_before(SignalId::new(*signal), secs(from))
            .unwrap();
        let want = nearest_brute(&all, *signal, from);
        // At or after the signal's last sample the summary reads just one
        // segment; the contract permits any chunk ending at that timestamp.
        let tied_last = got.zip(want).is_some_and(|(got, want)| {
            got.max_ts() == want.max_ts()
                && want.max_ts() <= secs(from)
                && got.chunk.signal == SignalId::new(*signal)
                && all.contains(&got)
                && all
                    .iter()
                    .filter(|c| c.chunk.signal == SignalId::new(*signal))
                    .all(|c| c.max_ts() <= want.max_ts())
        });
        assert!(
            got == want || tied_last,
            "nearest of {signal} at {from}: {got:?} != {want:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn every_answer_matches_a_brute_force_scan(
        sets in specs(),
        from in -100i64..2_500,
        span in 0i64..1_500,
        signals in prop::collection::vec(0u64..14, 1..5),
    ) {
        let s = Scratch::new();
        for (seq, set) in sets.iter().enumerate() {
            let chunks: Vec<_> = set.iter().map(|c| chunk(c.0, c.1, c.2, c.3)).collect();
            sealed(&s.dir(), seq as u64, &chunks);
        }
        check_answers(&s, from, span, &signals);
    }

    #[test]
    fn answers_and_liveness_match_after_random_compactions(
        sets in specs(),
        cuts in prop::collection::vec((0usize..6, 1usize..4), 0..3),
        from in 0i64..2_000,
        signals in prop::collection::vec(0u64..12, 1..4),
    ) {
        let s = Scratch::new();
        for (seq, set) in sets.iter().enumerate() {
            let chunks: Vec<_> = set.iter().map(|c| chunk(c.0, c.1, c.2, c.3)).collect();
            sealed(&s.dir(), seq as u64, &chunks);
        }
        let opts = StorageOptions::default();
        for (at, len) in cuts {
            let live = live_segments(&s.dir()).unwrap();
            let inputs: Vec<_> = live.iter().skip(at).take(len).map(|e| e.path.clone()).collect();
            if !inputs.is_empty() {
                compact(&s.dir(), &inputs, &opts).unwrap();
            }
        }
        check_answers(&s, from, 300, &signals);
        let live = live_segments(&s.dir()).unwrap().len();
        prop_assert_eq!(open(&s).snapshot().segments(STREAM).len(), live);
    }
}

#[test]
fn a_plan_comes_out_in_total_order_across_overlapping_segments() {
    let s = Scratch::new();
    let a: Vec<_> = (0..30).map(|i| chunk(i % 3, i as i64 * 10, 5, 1)).collect();
    let b: Vec<_> = (0..30).map(|i| chunk(i % 3, i as i64 * 7, 5, 1)).collect();
    sealed(&s.dir(), 0, &a);
    sealed(&s.dir(), 1, &b);
    let got = plan(&open(&s).snapshot(), PlanRequest::everything());
    assert_eq!(got.len(), 60);
    assert!(got.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(keys(&got), keys(&brute(&s.dir())));
}
