//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! SPEC §15's year, extrapolated from what this run measured: 16,000 signals at
//! 1 Hz for 365 days is 504,576,000,000 samples. Every figure is a product or a
//! quotient of a measured median and that count, and [`ASSUMPTIONS`] says what
//! makes it one. Pure, so `crates/server/tests/bench_matrix.rs` tests it.

use crate::stats::Metrics;

/// SPEC §15's signals.
pub const SIGNALS: f64 = 16_000.0;
/// 365 days of seconds; no leap day.
pub const SECONDS_PER_YEAR: f64 = 365.0 * 86_400.0;
/// 504,576,000,000.
pub const SAMPLES_PER_YEAR: f64 = SIGNALS * SECONDS_PER_YEAR;

/// The medians the extrapolation reads.
pub const COMPACTED: &str = "history.compaction.disk_bytes_per_sample";
pub const INGESTED: &str = "history.load.disk_bytes_per_sample";
pub const COMPACTION_RATE: &str = "history.compaction.samples_per_s";
/// Every ingest row's achieved rate ends in this.
pub const ACHIEVED: &str = ".achieved_per_s";

/// What the figures take for granted. Printed with them, always.
pub const ASSUMPTIONS: &[&str] = &[
    "Linear in samples: bytes per sample and compaction speed measured on this run's \
     data set hold for a year of it. Index sidecars and metadata grow with segments and \
     signals, not samples, and are included only as far as the data set had them.",
    "The value distribution is the harness's: a sawtooth in 0.1 steps with up to about 1.0 \
     of noise in 0.01 steps, as f64. Real populations compress better (flat setpoints, booleans) or worse (raw \
     noise); docs/reference/storage.md has per-shape figures.",
    "One stream, no retention, no replicas, no filesystem overhead beyond file sizes.",
    "The highest achieved ingest rate is a lower bound on capacity unless that row was \
     saturated: an offered rate that was met says nothing about the ceiling.",
    "The client ran on the same machine as the daemon and shared its CPUs.",
];

/// One figure of the year.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub label: &'static str,
    pub value: f64,
    pub unit: &'static str,
}

/// The year's figures; any whose inputs this run did not measure are absent.
pub fn extrapolate(metrics: &Metrics) -> Vec<Figure> {
    let mut out = vec![Figure {
        label: "samples a year (16,000 signals x 1 Hz x 365 days)",
        value: SAMPLES_PER_YEAR,
        unit: "samples",
    }];
    let tb = |b: f64| b * SAMPLES_PER_YEAR / 1e12;
    if let Some(b) = metrics.median_of(COMPACTED) {
        out.push(Figure {
            label: "disk a year, compacted",
            value: tb(b),
            unit: "TB",
        });
    }
    if let Some(b) = metrics.median_of(INGESTED) {
        out.push(Figure {
            label: "disk a year, never compacted (ingest layout)",
            value: tb(b),
            unit: "TB",
        });
    }
    if let Some(peak) = highest_ingest(metrics) {
        out.push(Figure {
            label: "highest achieved ingest rate, as a multiple of 16,000/s",
            value: peak / SIGNALS,
            unit: "x",
        });
        out.push(Figure {
            label: "time to ingest a year's backlog at that rate",
            value: SAMPLES_PER_YEAR / peak / 3_600.0,
            unit: "hours",
        });
    }
    if let Some(rate) = metrics.median_of(COMPACTION_RATE).filter(|r| *r > 0.0) {
        out.push(Figure {
            label: "compaction time a year of data needs",
            value: SAMPLES_PER_YEAR / rate / 3_600.0,
            unit: "hours",
        });
        out.push(Figure {
            label: "compaction time a day of data needs",
            value: SIGNALS * 86_400.0 / rate,
            unit: "s",
        });
    }
    out
}

/// The highest median achieved rate of any ingest row.
pub fn highest_ingest(metrics: &Metrics) -> Option<f64> {
    metrics
        .series
        .keys()
        .filter(|k| k.starts_with("ingest.") && k.ends_with(ACHIEVED))
        .filter_map(|k| metrics.median_of(k))
        .filter(|r| *r > 0.0)
        .reduce(f64::max)
}
