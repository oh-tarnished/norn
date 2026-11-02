//! NaN, infinities, signed zero and counter resets, exactly as documented.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, at, drain, rig, sample, store, target};
use norn_core::{Value, ValueKind};
use norn_query::{
    Filter, Function, Output, QueryError, QueryLimits, QuerySpec, Reduction, Results, TimeRange,
};

const SEC: i64 = 1_000_000_000;

/// The result of `function` over `values`, one a second, in one window.
fn one(values: &[Value], function: Function) -> Option<(Value, u64)> {
    let s = Scratch::new();
    let kind = values[0].kind();
    let list: Vec<_> = (0i64..)
        .zip(values)
        .map(|(i, v)| sample(1, i * SEC, v.clone()))
        .collect();
    store(&s.dir(), &[list], 1000);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let spec = QuerySpec {
        targets: vec![target(1, kind)],
        range: TimeRange::default(),
        filter: Filter::default(),
        output: Output::Aggregates(Reduction {
            function,
            window: None,
        }),
        resume: None,
    };
    let mut query = r.engine.open(spec).unwrap();
    let mut pages = drain(&mut query).unwrap();
    let page = pages.pop()?;
    let Results::Aggregates(mut list) = page.results else {
        panic!()
    };
    list.pop().map(|a| (a.value, a.samples))
}

fn float(values: &[f64], function: Function) -> f64 {
    let values: Vec<_> = values.iter().map(|v| Value::F64(*v)).collect();
    match one(&values, function).unwrap().0 {
        Value::F64(v) => v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn min_and_max_use_total_order_so_a_nan_outranks_every_number() {
    let nan = f64::NAN;
    assert_eq!(float(&[1.0, nan, 2.0], Function::Min), 1.0);
    assert!(float(&[1.0, nan, 2.0], Function::Max).is_nan());
    let negative_nan = -f64::NAN;
    assert!(float(&[1.0, negative_nan], Function::Min).is_nan());
    assert_eq!(float(&[1.0, negative_nan], Function::Max), 1.0);
}

#[test]
fn min_and_max_tell_negative_zero_from_positive_in_either_order() {
    for values in [[0.0, -0.0], [-0.0, 0.0]] {
        assert_eq!(float(&values, Function::Min).to_bits(), (-0.0f64).to_bits());
        assert_eq!(float(&values, Function::Max).to_bits(), 0.0f64.to_bits());
    }
}

#[test]
fn infinities_order_inside_the_nans_and_outside_the_numbers() {
    let values = [1.0, f64::INFINITY, f64::NEG_INFINITY];
    assert_eq!(float(&values, Function::Min), f64::NEG_INFINITY);
    assert_eq!(float(&values, Function::Max), f64::INFINITY);
}

#[test]
fn sum_and_average_follow_ieee_754() {
    assert!(float(&[1.0, f64::NAN], Function::Sum).is_nan());
    assert!(float(&[1.0, f64::NAN], Function::Average).is_nan());
    assert_eq!(float(&[1.0, f64::INFINITY], Function::Sum), f64::INFINITY);
    assert!(float(&[f64::INFINITY, f64::NEG_INFINITY], Function::Sum).is_nan());
    assert!(float(&[f64::INFINITY, f64::NEG_INFINITY], Function::Average).is_nan());
}

#[test]
fn a_sum_of_negative_zeros_stays_negative_zero() {
    let sum = float(&[-0.0, -0.0], Function::Sum);
    assert_eq!(sum.to_bits(), (-0.0f64).to_bits());
    assert_eq!(
        float(&[-0.0, 0.0], Function::Sum).to_bits(),
        0.0f64.to_bits()
    );
}

#[test]
fn integer_sums_are_exact_past_where_a_double_would_lose_them() {
    let big = Value::I64(i64::MAX);
    let values = [big.clone(), big, Value::I64(-i64::MAX)];
    let (sum, _) = one(&values, Function::Sum).unwrap();
    assert_eq!(sum, Value::F64(i64::MAX as f64));
}

#[test]
fn rate_counts_a_decrease_as_a_reset_from_zero() {
    // 10 → 30 gains 20; the reset to 5 gains 5; 5 → 15 gains 10: 35 over 3 s.
    let rate = float(&[10.0, 30.0, 5.0, 15.0], Function::Rate);
    assert!((rate - 35.0 / 3.0).abs() < 1e-12, "{rate}");
    let values = [
        Value::U64(u64::MAX - 1),
        Value::U64(u64::MAX),
        Value::U64(1),
    ];
    let (Value::F64(rate), samples) = one(&values, Function::Rate).unwrap() else {
        panic!()
    };
    assert_eq!((rate, samples), (1.0, 3));
}

#[test]
fn a_steady_counter_has_rate_zero_and_a_lone_sample_has_none() {
    assert_eq!(float(&[4.0, 4.0, 4.0], Function::Rate), 0.0);
    assert_eq!(one(&[Value::F64(1.0)], Function::Rate), None);
}

#[test]
fn a_nan_poisons_a_rate() {
    assert!(float(&[1.0, f64::NAN, 3.0], Function::Rate).is_nan());
}

#[test]
fn value_predicates_on_a_nan_hold_only_for_not_equal() {
    let f = Filter::parse("value > 1").unwrap();
    assert!(!f.passes_value(&Value::F64(f64::NAN)));
    let f = Filter::parse("value != 1").unwrap();
    assert!(f.passes_value(&Value::F64(f64::NAN)));
    let f = Filter::parse("value = 0").unwrap();
    assert!(f.passes_value(&Value::F64(-0.0)));
}

#[test]
fn min_and_max_work_on_strings_and_refuse_nothing_ordered() {
    let values = [Value::String("b".into()), Value::String("a".into())];
    assert_eq!(one(&values, Function::Min).unwrap().0, values[1]);
    assert_eq!(one(&values, Function::Count).unwrap().0, Value::I64(2));
}

#[test]
fn sum_on_a_string_signal_or_a_mismatched_filter_is_refused_before_any_work() {
    let s = Scratch::new();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let spec = |filter: &str, function| QuerySpec {
        targets: vec![target(1, ValueKind::String)],
        range: TimeRange {
            start: Some(at(0)),
            end: None,
        },
        filter: Filter::parse(filter).unwrap(),
        output: Output::Aggregates(Reduction {
            function,
            window: None,
        }),
        resume: None,
    };
    let refused = |r: Result<_, QueryError>| matches!(r, Err(QueryError::NotApplicable { .. }));
    assert!(refused(r.engine.open(spec("", Function::Sum))));
    assert!(refused(r.engine.open(spec("value > 3", Function::Count))));
    assert!(
        r.engine
            .open(spec("value > \"a\"", Function::Count))
            .is_ok()
    );
}

#[test]
fn a_filter_refuses_what_it_cannot_read() {
    for text in [
        "bogus = 1",
        "value",
        "value > nope",
        "quality.severity = GOOD",
        "value > 1 OR value < 0",
    ] {
        assert!(Filter::parse(text).is_err(), "{text}");
    }
}
