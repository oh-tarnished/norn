//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Raw ranges and LATEST equal a brute-force scan, whatever the layout of the data.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::oracle::*;
use common::{Scratch, all_seen, at, drain, target};
use norn_core::ValueKind;
use norn_query::{Function, Order, Output, Reduction, Results, TimeRange};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn raw_ranges_match_a_brute_force_scan_in_both_orders(
        raw in raw(), shape in layout(), a in 0i64..400, b in 0i64..400, (text, keep) in filters(),
    ) {
        let s = Scratch::new();
        let all = samples(&raw);
        let r = lay_out(&s, &rig0(shape), &all);
        let (from, to) = (a.min(b), a.max(b));
        let range = TimeRange { start: Some(at(from)), end: Some(at(to)) };
        let want = oracle(&all, from, to, keep);

        let got = all_seen(&r.engine, spec(range, text, Output::Samples(Order::BySignal)));
        let mut by_signal = want.clone();
        by_signal.sort_by_key(|s| (s.signal.get(), s.timestamp.as_nanos(), s.sequence.get()));
        let rows: Vec<_> = got.into_iter().map(got_row).collect();
        prop_assert_eq!(rows, by_signal.iter().map(expected_row).collect::<Vec<_>>());

        let got = all_seen(&r.engine, spec(range, text, Output::Samples(Order::ByTime)));
        let mut by_time = want;
        by_time.sort_by_key(key_time);
        let rows: Vec<_> = got.into_iter().map(got_row).collect();
        prop_assert_eq!(rows, by_time.iter().map(expected_row).collect::<Vec<_>>());
    }

    #[test]
    fn latest_is_the_newest_match_of_each_signal(
        raw in raw(), shape in layout(), a in 0i64..400, b in 0i64..400, (text, keep) in filters(),
    ) {
        let s = Scratch::new();
        let all = samples(&raw);
        let r = lay_out(&s, &rig0(shape), &all);
        let (from, to) = (a.min(b), a.max(b));
        let range = TimeRange { start: Some(at(from)), end: Some(at(to)) };
        let reduction = Reduction { function: Function::Latest, window: None };
        let mut query = r.engine.open(spec(range, text, Output::Aggregates(reduction))).unwrap();
        let mut got = Vec::new();
        for page in drain(&mut query).unwrap() {
            let Results::Aggregates(list) = page.results else { panic!() };
            got.extend(list.into_iter().map(|a| (a.signal, a.start.unwrap().as_nanos(), number(&a.value))));
        }
        let mut want = Vec::new();
        for signal in 0..SIGNALS {
            let mut m = oracle(&all, from, to, keep);
            m.retain(|s| s.signal.get() == signal);
            if let Some(x) = m.iter().max_by_key(|s| key_time(s)) {
                let name = target(signal, ValueKind::F64).name;
                want.push((name, x.timestamp.as_nanos(), number(&x.value)));
            }
        }
        prop_assert_eq!(got, want);
    }
}
