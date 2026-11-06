//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! CI's regression check: a run against a committed baseline, which is a report
//! this harness wrote earlier (`benchmarks/baseline/quick.json`).
//!
//! A baseline is machine-specific. Only gated metrics are compared, by median,
//! and a metric fails only when it is worse than the baseline by more than the
//! threshold factor (3 by default: generous, because shared runners vary). A
//! check that compared nothing fails, so a renamed metric or an empty baseline
//! cannot pass silently. Pure, so `crates/server/tests/bench_matrix.rs` tests it.

use serde_json::Value;

use crate::stats::{Better, Metrics};

/// Format version of a report; a baseline of any other is refused.
pub const SCHEMA: &str = "norn-bench-matrix/1";

/// The default factor a gated median may worsen by before CI fails.
pub const THRESHOLD: f64 = 3.0;

/// What the comparison found.
#[derive(Debug, Default)]
pub struct Verdict {
    pub compared: usize,
    pub regressions: Vec<String>,
    pub notes: Vec<String>,
}

impl Verdict {
    /// Passed: something was compared, and nothing regressed.
    pub fn passed(&self) -> bool {
        self.compared > 0 && self.regressions.is_empty()
    }
}

/// How many times worse `now` is than `base`; below 1 is an improvement.
/// `None` when the baseline is not positive, so no ratio means anything.
pub fn worsening(better: Better, base: f64, now: f64) -> Option<f64> {
    if base <= 0.0 || !base.is_finite() || !now.is_finite() {
        return None;
    }
    Some(match better {
        Better::Lower => now / base,
        Better::Higher if now <= 0.0 => f64::INFINITY,
        Better::Higher => base / now,
    })
}

/// Machine fields that, when they differ, make the comparison a weak signal.
const SAME_MACHINE: [&str; 3] = ["os", "arch", "cpu"];

/// Compares `run` (of `profile`, measured on `machine`) with `baseline`.
pub fn compare(
    run: &Metrics,
    profile: &str,
    machine: &Value,
    baseline: &Value,
    threshold: f64,
) -> Verdict {
    let mut verdict = Verdict::default();
    if baseline["schema"].as_str() != Some(SCHEMA) {
        let found = &baseline["schema"];
        verdict
            .notes
            .push(format!("the baseline's schema is {found}, not {SCHEMA}"));
        return verdict;
    }
    if baseline["profile"].as_str() != Some(profile) {
        let found = &baseline["profile"];
        verdict.notes.push(format!(
            "the baseline's profile is {found}, this run's is {profile}: different scales"
        ));
        return verdict;
    }
    for field in SAME_MACHINE {
        let (was, is) = (&baseline["machine"][field], &machine[field]);
        if was != is {
            verdict.notes.push(format!(
                "baseline {field} is {was}, this machine's is {is}: the baseline is \
                 machine-specific, so read a failure as a prompt to look, not a proof"
            ));
        }
    }
    let Some(metrics) = baseline["metrics"].as_object() else {
        verdict
            .notes
            .push("the baseline has no `metrics` object".into());
        return verdict;
    };
    for (key, entry) in metrics {
        if entry["gated"].as_bool() != Some(true) {
            continue;
        }
        let better = entry["better"].as_str().and_then(Better::parse);
        let (Some(better), Some(base)) = (better, entry["median"].as_f64()) else {
            verdict
                .notes
                .push(format!("{key}: unreadable in the baseline"));
            continue;
        };
        let Some(now) = run.median_of(key) else {
            // Absent: `--only` or a rename. Present but empty: it stopped working.
            if run.series.contains_key(key) {
                verdict
                    .regressions
                    .push(format!("{key}: no value this run, baseline {base:.4}"));
            } else {
                let note = format!("{key}: in the baseline, not measured now");
                verdict.notes.push(note);
            }
            continue;
        };
        let Some(factor) = worsening(better, base, now) else {
            verdict
                .notes
                .push(format!("{key}: baseline {base} cannot be compared"));
            continue;
        };
        verdict.compared += 1;
        if factor > threshold {
            verdict.regressions.push(format!(
                "{key}: {now:.4} against a baseline of {base:.4} ({} is better), \
                 {factor:.2}x worse, over the {threshold}x threshold",
                better.word()
            ));
        }
    }
    verdict
}
