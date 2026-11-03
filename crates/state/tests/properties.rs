//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! SPEC §36: `StateAt(T)` equals the state from replaying every event up to T.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "../../query/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use common::oracle::*;
use common::{Scratch, at, drain, rig, store, target};
use norn_core::{Sample, ValueKind};
use norn_query::{Filter, Order, Output, QueryLimits, QuerySpec, Results, Target, TimeRange};
use norn_state::StateReader;
use proptest::prelude::*;

fn targets() -> Vec<Target> {
    (0..SIGNALS).map(|s| target(s, ValueKind::F64)).collect()
}

/// (signal, time, sequence, value, quality): everything a state must get right.
type Row = (String, i64, u64, f64, u16);

fn row(name: &str, s: &Sample) -> Row {
    let (t, q) = (s.timestamp.as_nanos(), s.sequence.get());
    let quality = (s.quality.severity() as u16) << 8 | s.quality.substatus() as u16;
    (name.to_owned(), t, q, number(&s.value), quality)
}

/// Every signal's newest sample at or before `t`, by looking at all of them.
fn brute(all: &[Sample], t: i64, oldest: i64) -> Vec<Row> {
    let mut rows = Vec::new();
    for signal in 0..SIGNALS {
        let of = all.iter().filter(|s| s.signal.get() == signal);
        let inside = of.filter(|s| (oldest..=t).contains(&s.timestamp.as_nanos()));
        if let Some(s) = inside.max_by_key(|s| key_time(s)) {
            rows.push(row(&target(signal, ValueKind::F64).name, s));
        }
    }
    rows
}

/// Deals the samples among two segments and the tail, each in order, so chunks of
/// one signal overlap each other and the buffers, and some sit in both.
fn overlapping(s: &Scratch, all: &[Sample], deal: u64, chunk: usize, slice: u64) -> common::Rig {
    let mut piles: [Vec<Sample>; 3] = Default::default();
    for (i, sample) in all.iter().enumerate() {
        let hand = (i as u64 ^ deal).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 62;
        piles[(hand % 3) as usize].push(sample.clone());
        if hand == 3 {
            piles[2].push(sample.clone());
        }
    }
    let [one, two, tail] = piles;
    store(&s.dir(), &[one, two], chunk);
    let mut limits = QueryLimits::default();
    limits.slice_bytes = slice;
    let rigged = rig(&s.0, 1 << 30, limits);
    *rigged.tail.0.lock().unwrap() = tail;
    rigged
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn state_at_matches_a_brute_force_scan_and_a_replay(
        raw in raw(), deal: u64, chunk in 1usize..30, slice in 256u64..20_000,
        t in 0i64..420, age in 0i64..420, aged: bool,
    ) {
        let s = Scratch::new();
        let all = samples(&raw);
        let r = overlapping(&s, &all, deal, chunk, slice);
        let reader = StateReader::new(r.engine.clone());
        let max_age = aged.then_some(age);
        let state = reader.state_at(targets(), at(t), max_age).unwrap();
        prop_assert_eq!(state.entries().len(), SIGNALS as usize);
        let got: Vec<Row> = state.entries().iter()
            .filter_map(|e| e.sample.as_ref().map(|x| row(&e.signal, x)))
            .collect();
        let oldest = max_age.map_or(i64::MIN, |a| t - a);
        prop_assert_eq!(&got, &brute(&all, t, oldest));

        // The same thing by replaying: fold the replay order up to and including T.
        let range = TimeRange { start: None, end: Some(at(t + 1)) };
        let spec = QuerySpec {
            targets: targets(),
            range,
            filter: Filter::default(),
            output: Output::Samples(Order::ByTime),
            resume: None,
        };
        let mut query = r.engine.open(spec).unwrap();
        let mut folded: BTreeMap<String, Row> = BTreeMap::new();
        for page in drain(&mut query).unwrap() {
            let Results::Samples(list) = page.results else { panic!() };
            for (name, sample) in &list {
                folded.insert(name.to_string(), row(name, sample));
            }
        }
        folded.retain(|_, r| r.1 >= oldest);
        prop_assert_eq!(got, folded.into_values().collect::<Vec<_>>());
    }
}
