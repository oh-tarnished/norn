//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The run's two outputs: JSON for machines (and the baseline), and a Markdown
//! table for docs/performance.md. Both carry the machine, the commit and the
//! configuration; neither is written without them.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};

use crate::baseline::SCHEMA;
use crate::stats::{Metrics, summarise};
use crate::yearly::{ASSUMPTIONS, Figure};

/// The JSON report. `context` holds profile, machine, commit and config.
pub fn json(context: Map<String, Value>, metrics: &Metrics, year: &[Figure]) -> Value {
    let mut all = Map::new();
    for (key, s) in &metrics.series {
        let mut entry = json!({
            "unit": s.unit,
            "better": s.better.word(),
            "gated": s.gated,
            "values": s.values,
        });
        if let Some(sum) = summarise(&s.values) {
            entry["median"] = json!(sum.median);
            entry["p95"] = json!(sum.p95);
            entry["min"] = json!(sum.min);
            entry["max"] = json!(sum.max);
        }
        all.insert(key.clone(), entry);
    }
    let yearly: Vec<Value> = year
        .iter()
        .map(|f| json!({"label": f.label, "value": f.value, "unit": f.unit}))
        .collect();
    let mut out = Map::new();
    out.insert("schema".into(), json!(SCHEMA));
    out.extend(context);
    out.insert("metrics".into(), Value::Object(all));
    out.insert(
        "yearly".into(),
        json!({"figures": yearly, "assumptions": ASSUMPTIONS}),
    );
    Value::Object(out)
}

/// Four significant figures or so, without exponent noise.
pub fn number(v: f64) -> String {
    match v.abs() {
        a if a >= 1e5 => format!("{:.0}", v),
        a if a >= 100.0 => format!("{v:.1}"),
        a if a >= 1.0 => format!("{v:.3}"),
        _ => format!("{v:.4}"),
    }
}

fn text(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Null => "unknown".into(),
        other => other.to_string(),
    }
}

/// The Markdown report, from the JSON one so the two cannot disagree.
pub fn markdown(report: &Value) -> String {
    let mut md = String::new();
    let m = &report["machine"];
    let c = &report["commit"];
    let _ = writeln!(
        md,
        "# Norn benchmark matrix: {} profile\n",
        text(report, "profile")
    );
    let _ = writeln!(
        md,
        "- Machine: {}, {} logical CPUs, {} bytes of RAM, {} ({})",
        text(m, "cpu"),
        text(m, "logical_cpus"),
        text(m, "ram_bytes"),
        text(m, "os"),
        text(m, "arch")
    );
    let _ = writeln!(
        md,
        "- Disk: {}, {} filesystem, data under `{}`",
        text(m, "disk_model"),
        text(m, "filesystem"),
        text(m, "data_root")
    );
    let _ = writeln!(
        md,
        "- Commit: `{}` ({}), dirty: {}; {}",
        text(c, "commit"),
        text(c, "describe"),
        text(c, "dirty"),
        text(m, "rustc")
    );
    let _ = writeln!(
        md,
        "- Repeats: {}; each cell is over the repeats, nearest-rank p95",
        text(report, "repeats")
    );
    let _ = writeln!(
        md,
        "- Daemon configuration:\n\n```toml\n{}```\n",
        report["config"]["daemon_toml"].as_str().unwrap_or("")
    );
    let _ = writeln!(
        md,
        "| Metric | Unit | Better | Runs | Median | p95 | Min | Max |"
    );
    let _ = writeln!(md, "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |");
    for (key, e) in report["metrics"].as_object().into_iter().flatten() {
        let cell = |k: &str| e[k].as_f64().map_or("-".into(), number);
        let runs = e["values"].as_array().map_or(0, Vec::len);
        let gate = if e["gated"].as_bool() == Some(true) {
            " (gated)"
        } else {
            ""
        };
        let _ = writeln!(
            md,
            "| `{key}`{gate} | {} | {} | {runs} | {} | {} | {} | {} |",
            text(e, "unit"),
            text(e, "better"),
            cell("median"),
            cell("p95"),
            cell("min"),
            cell("max")
        );
    }
    let _ = writeln!(
        md,
        "\n## The year, extrapolated\n\n| Figure | Value | Unit |\n| --- | ---: | --- |"
    );
    for f in report["yearly"]["figures"].as_array().into_iter().flatten() {
        let value = f["value"].as_f64().map_or("-".into(), number);
        let _ = writeln!(
            md,
            "| {} | {value} | {} |",
            text(f, "label"),
            text(f, "unit")
        );
    }
    let _ = writeln!(md, "\nAssumptions:\n");
    for a in ASSUMPTIONS {
        let _ = writeln!(md, "- {a}");
    }
    md
}
