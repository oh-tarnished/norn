//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The benchmark matrix's arithmetic: summaries, the regression check and the
//! yearly extrapolation. Pure, with inputs written for these tests: nothing here
//! is a measurement. The harness itself is smoke-run by `just bench-smoke`.
#![allow(missing_docs, clippy::unwrap_used, clippy::indexing_slicing)]

#[path = "../benches/matrix/baseline.rs"]
#[allow(dead_code)]
mod baseline;
#[path = "../benches/matrix/stats.rs"]
#[allow(dead_code)]
mod stats;
#[path = "../benches/matrix/yearly.rs"]
#[allow(dead_code)]
mod yearly;

use serde_json::{Value, json};

use baseline::{SCHEMA, compare, worsening};
use stats::{Better, Metrics, median, percentile, summarise};
use yearly::{SAMPLES_PER_YEAR, extrapolate};

#[test]
fn median_takes_the_middle_or_the_mean_of_the_middle_two() {
    assert_eq!(median(&[1.0, 2.0, 9.0]), Some(2.0));
    assert_eq!(median(&[1.0, 2.0, 4.0, 9.0]), Some(3.0));
    assert_eq!(median(&[]), None);
}

#[test]
fn percentile_is_nearest_rank() {
    let values: Vec<f64> = (1..=100).map(f64::from).collect();
    assert_eq!(percentile(&values, 0.95), Some(95.0));
    assert_eq!(percentile(&[1.0, 2.0, 3.0], 0.95), Some(3.0));
    assert_eq!(percentile(&[1.0, 2.0, 3.0], 0.0), Some(1.0));
    assert_eq!(percentile(&[], 0.5), None);
}

#[test]
fn summary_sorts_and_nothing_measured_has_no_summary() {
    let s = summarise(&[3.0, 1.0, 2.0]).unwrap();
    assert_eq!(
        (s.runs, s.median, s.p95, s.min, s.max),
        (3, 2.0, 3.0, 1.0, 3.0)
    );
    assert_eq!(summarise(&[]), None);
}

#[test]
fn a_non_finite_value_is_kept_out_of_a_series() {
    let mut m = Metrics::default();
    m.record("k", "ms", Better::Lower, true, f64::NAN);
    assert_eq!(m.median_of("k"), None);
    m.record("k", "ms", Better::Lower, true, f64::INFINITY);
    m.record("k", "ms", Better::Lower, true, 2.0);
    assert_eq!(m.median_of("k"), Some(2.0));
}

#[test]
fn worsening_follows_the_direction_of_the_metric() {
    assert_eq!(worsening(Better::Lower, 2.0, 6.0), Some(3.0));
    assert_eq!(worsening(Better::Higher, 6.0, 2.0), Some(3.0));
    assert_eq!(worsening(Better::Higher, 6.0, 0.0), Some(f64::INFINITY));
    assert_eq!(worsening(Better::Lower, 0.0, 6.0), None);
}

fn base(key: &str, better: &str, gated: bool, value: f64) -> Value {
    json!({
        "schema": SCHEMA,
        "profile": "quick",
        "machine": {"os": "a", "arch": "b", "cpu": "c"},
        "metrics": {key: {"better": better, "gated": gated, "median": value}},
    })
}

fn run(key: &str, better: Better, value: f64) -> Metrics {
    let mut m = Metrics::default();
    m.record(key, "u", better, true, value);
    m
}

fn same_machine() -> Value {
    json!({"os": "a", "arch": "b", "cpu": "c"})
}

fn check(m: &Metrics, b: &Value) -> baseline::Verdict {
    compare(m, "quick", &same_machine(), b, 3.0)
}

#[test]
fn within_the_threshold_passes_and_beyond_it_fails() {
    let b = base("k", "lower", true, 10.0);
    let v = check(&run("k", Better::Lower, 30.0), &b);
    assert!(v.passed() && v.compared == 1 && v.notes.is_empty(), "{v:?}");
    let v = check(&run("k", Better::Lower, 30.1), &b);
    assert!(!v.passed() && v.regressions.len() == 1, "{v:?}");
}

#[test]
fn a_fall_in_a_rate_is_a_regression_and_a_rise_is_not() {
    let b = base("k", "higher", true, 90.0);
    assert!(check(&run("k", Better::Higher, 1_000.0), &b).passed());
    assert!(!check(&run("k", Better::Higher, 29.0), &b).passed());
}

#[test]
fn comparing_nothing_fails() {
    let ungated = base("k", "lower", false, 10.0);
    assert!(!check(&run("k", Better::Lower, 1.0), &ungated).passed());
    let other = base("renamed", "lower", true, 10.0);
    let v = check(&run("k", Better::Lower, 1.0), &other);
    assert!(!v.passed() && v.notes.len() == 1, "{v:?}");
}

#[test]
fn another_schema_or_profile_is_refused() {
    let m = run("k", Better::Lower, 1.0);
    let mut b = base("k", "lower", true, 10.0);
    b["schema"] = json!("something-else/9");
    assert!(!check(&m, &b).passed());
    let mut b = base("k", "lower", true, 10.0);
    b["profile"] = json!("smoke");
    assert!(!check(&m, &b).passed());
}

#[test]
fn a_metric_that_stopped_producing_values_is_a_regression() {
    let b = base("k", "lower", true, 10.0);
    let mut m = run("other", Better::Lower, 1.0);
    m.record("k", "ms", Better::Lower, true, f64::NAN);
    let v = check(&m, &b);
    assert!(!v.passed() && v.regressions.len() == 1, "{v:?}");
}

#[test]
fn another_machine_is_noted_not_failed() {
    let b = base("k", "lower", true, 10.0);
    let elsewhere = json!({"os": "z", "arch": "b", "cpu": "c"});
    let v = compare(&run("k", Better::Lower, 10.0), "quick", &elsewhere, &b, 3.0);
    assert!(v.passed() && v.notes.len() == 1, "{v:?}");
}

#[test]
fn the_year_is_the_figure_the_specification_states() {
    assert_eq!(SAMPLES_PER_YEAR, 504_576_000_000.0);
}

#[test]
fn the_year_follows_from_its_inputs_and_is_absent_without_them() {
    assert_eq!(extrapolate(&Metrics::default()).len(), 1);
    let mut m = Metrics::default();
    m.record(yearly::COMPACTED, "b", Better::Lower, true, 2.0);
    m.record(
        yearly::COMPACTION_RATE,
        "r",
        Better::Higher,
        false,
        16_000.0,
    );
    m.record(
        "ingest.a.achieved_per_s",
        "r",
        Better::Higher,
        true,
        32_000.0,
    );
    m.record(
        "ingest.b.achieved_per_s",
        "r",
        Better::Higher,
        true,
        16_000.0,
    );
    m.record("planes.c.achieved_per_s", "r", Better::Higher, true, 1e9);
    let year = extrapolate(&m);
    let figure = |label: &str| year.iter().find(|f| f.label.contains(label)).unwrap().value;
    assert_eq!(
        figure("disk a year, compacted"),
        2.0 * 504_576_000_000.0 / 1e12
    );
    assert_eq!(figure("multiple"), 2.0);
    assert_eq!(figure("compaction time a day"), 86_400.0);
}
