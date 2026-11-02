//! Every reduction over windows equals a brute-force scan.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::collections::BTreeMap;

use common::oracle::*;
use common::{Scratch, at, drain, target};
use norn_core::{Sample, Value, ValueKind};
use norn_query::{Function, Output, Reduction, Results, TimeRange};
use proptest::prelude::*;

/// What a reduction over `window` of `samples` must give: (window start, result).
fn reduce(f: Function, samples: &[&Sample]) -> Option<f64> {
    let vals: Vec<f64> = samples
        .iter()
        .map(|s| match s.value {
            Value::F64(v) => v,
            _ => panic!(),
        })
        .collect();
    let times = |i: usize| samples[i].timestamp.as_nanos();
    Some(match f {
        Function::First => *vals.first()?,
        Function::Last => *vals.last()?,
        Function::Count => vals.len() as f64,
        Function::Min => vals.iter().copied().min_by(f64::total_cmp)?,
        Function::Max => vals.iter().copied().max_by(f64::total_cmp)?,
        Function::Sum => vals.iter().copied().reduce(|a, b| a + b)?,
        Function::Average => vals.iter().copied().reduce(|a, b| a + b)? / vals.len() as f64,
        Function::Rate => {
            let span = (times(vals.len().checked_sub(1)?) - times(0)) as f64 / 1e9;
            if span <= 0.0 {
                return None;
            }
            let gain: f64 = vals
                .windows(2)
                .map(|w| if w[1] < w[0] { w[1] } else { w[1] - w[0] })
                .sum();
            gain / span
        }
        _ => unreachable!(),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn every_aggregate_over_windows_matches_a_brute_force_scan(
        raw in raw(), shape in layout(), width in 1i64..120, (text, keep) in filters(),
    ) {
        let s = Scratch::new();
        let all = samples(&raw);
        let r = lay_out(&s, &rig0(shape), &all);
        let (from, to) = (10, 390);
        let range = TimeRange { start: Some(at(from)), end: Some(at(to)) };
        for function in [
            Function::First, Function::Last, Function::Count, Function::Min, Function::Max,
            Function::Average, Function::Sum, Function::Rate,
        ] {
            let output = Output::Aggregates(Reduction { function, window: Some(width) });
            let mut query = r.engine.open(spec(range, text, output)).unwrap();
            let mut got = Vec::new();
            for page in drain(&mut query).unwrap() {
                let Results::Aggregates(list) = page.results else { panic!() };
                got.extend(list);
            }
            let mut want = Vec::new();
            for signal in 0..SIGNALS {
                let mut matching = oracle(&all, from, to, keep);
                matching.retain(|s| s.signal.get() == signal);
                matching.sort_by_key(key_time);
                let mut windows: BTreeMap<i64, Vec<&Sample>> = BTreeMap::new();
                for x in &matching {
                    windows.entry((x.timestamp.as_nanos() - from) / width).or_default().push(x);
                }
                for (k, samples) in windows {
                    if let Some(v) = reduce(function, &samples) {
                        let name = target(signal, ValueKind::F64).name;
                        want.push((name, from + k * width, v, samples.len() as u64));
                    }
                }
            }
            let got: Vec<_> = got.iter().map(|a| {
                (a.signal.clone(), a.start.unwrap().as_nanos(), number(&a.value), a.samples)
            }).collect();
            prop_assert_eq!(got.len(), want.len(), "{:?}", function);
            for (g, w) in got.iter().zip(&want) {
                prop_assert_eq!((&g.0, g.1, g.3), (&w.0, w.1, w.3), "{:?}", function);
                prop_assert_eq!(g.2.to_bits(), w.2.to_bits(), "{:?}", function);
            }
        }
    }
}
