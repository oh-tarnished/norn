//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Repeated measurements and their summaries: every metric is measured once per
//! repeat, and reported as the median and the 95th percentile over the repeats.
//!
//! Pure: no clock, no I/O, so `crates/server/tests/bench_matrix.rs` tests it.

use std::collections::BTreeMap;

/// Which way a metric improves; the regression check needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Better {
    /// Throughput, a rate: a fall is a regression.
    Higher,
    /// Latency, time, size: a rise is a regression.
    Lower,
}

impl Better {
    pub fn word(self) -> &'static str {
        match self {
            Better::Higher => "higher",
            Better::Lower => "lower",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "higher" => Some(Better::Higher),
            "lower" => Some(Better::Lower),
            _ => None,
        }
    }
}

/// One metric's values, one per repeat that measured it.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub unit: String,
    pub better: Better,
    /// Whether CI's regression check compares it. Tails (p99) and memory are
    /// reported, not gated: on a shared runner they are mostly noise.
    pub gated: bool,
    pub values: Vec<f64>,
}

/// Every metric of a run, by key (`group.case.measure`), in key order.
#[derive(Debug, Default)]
pub struct Metrics {
    pub series: BTreeMap<String, Series>,
}

impl Metrics {
    /// Adds one repeat's value. A non-finite value is kept out: a division by a
    /// zero duration must not become the median.
    pub fn record(&mut self, key: &str, unit: &str, better: Better, gated: bool, value: f64) {
        let entry = self.series.entry(key.to_owned()).or_insert_with(|| Series {
            unit: unit.to_owned(),
            better,
            gated,
            values: Vec::new(),
        });
        if value.is_finite() {
            entry.values.push(value);
        }
    }

    /// The median of `key`'s values, for derived figures such as the yearly
    /// extrapolation.
    pub fn median_of(&self, key: &str) -> Option<f64> {
        self.series
            .get(key)
            .and_then(|s| summarise(&s.values))
            .map(|s| s.median)
    }
}

/// What a series reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub runs: usize,
    pub median: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
}

/// Nearest-rank percentile of ascending `sorted`, `q` in 0..=1: the smallest
/// value with at least `q` of the values at or below it. With three repeats the
/// p95 is the largest, which is the honest reading of three numbers.
pub fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    let len = sorted.len();
    if len == 0 {
        return None;
    }
    let rank = (q.clamp(0.0, 1.0) * len as f64).ceil() as usize;
    sorted.get(rank.clamp(1, len) - 1).copied()
}

/// The median: the middle value, or the mean of the middle two.
pub fn median(sorted: &[f64]) -> Option<f64> {
    let len = sorted.len();
    let upper = sorted.get(len / 2).copied()?;
    if len % 2 == 1 {
        return Some(upper);
    }
    let lower = sorted.get(len / 2 - 1).copied()?;
    Some(f64::midpoint(lower, upper))
}

/// Sorts a copy of `values` and summarises it; `None` when nothing was measured.
pub fn summarise(values: &[f64]) -> Option<Summary> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(Summary {
        runs: sorted.len(),
        median: median(&sorted)?,
        p95: percentile(&sorted, 0.95)?,
        min: *sorted.first()?,
        max: *sorted.last()?,
    })
}
