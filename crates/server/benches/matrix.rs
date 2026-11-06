//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The benchmark matrix (stage 15): SPEC §15, §17 and §35's workload against
//! the real `norn`, over real gRPC on a Unix socket and over shared memory,
//! repeated, summarised, and written as JSON and Markdown.
//!
//! `just bench-matrix --quick` (CI, minutes) or `just bench-matrix --full` (the
//! owner, hours); `cargo test --benches` runs a smoke profile. What each row
//! does is in `matrix/profile.rs`; the method is in docs/performance.md. Every
//! daemon runs on a fresh directory under `--data-root` with its own socket,
//! removed when it ends. Unix only: the daemon's endpoints here are sockets.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[cfg(unix)]
#[path = "append/driver.rs"]
#[allow(dead_code)]
mod driver;
#[cfg(unix)]
#[path = "query/load.rs"]
mod load;
#[cfg(unix)]
#[path = "query/measure.rs"]
#[allow(dead_code)]
mod measure;
#[cfg(unix)]
#[path = "state/replaying.rs"]
mod replaying;
#[cfg(unix)]
#[path = "append/setup.rs"]
#[allow(dead_code)]
mod setup;
#[cfg(unix)]
#[path = "state/timing.rs"]
#[allow(dead_code)]
mod timing;

#[cfg(unix)]
#[path = "matrix/args.rs"]
mod args;
#[cfg(unix)]
#[path = "matrix/baseline.rs"]
mod baseline;
#[cfg(unix)]
#[path = "matrix/compact.rs"]
mod compact;
#[cfg(unix)]
#[path = "matrix/daemon.rs"]
mod daemon;
#[cfg(unix)]
#[path = "matrix/history.rs"]
mod history;
#[cfg(unix)]
#[path = "matrix/ingest.rs"]
mod ingest;
#[cfg(unix)]
#[path = "matrix/machine.rs"]
mod machine;
#[cfg(unix)]
#[path = "matrix/profile.rs"]
mod profile;
#[cfg(unix)]
#[path = "matrix/reads.rs"]
mod reads;
#[cfg(unix)]
#[path = "matrix/recovery.rs"]
mod recovery;
#[cfg(unix)]
#[path = "matrix/report.rs"]
mod report;
#[cfg(unix)]
#[path = "matrix/rows.rs"]
mod rows;
#[cfg(unix)]
#[path = "matrix/seed.rs"]
mod seed;
#[cfg(unix)]
#[path = "matrix/stats.rs"]
mod stats;
#[cfg(unix)]
#[path = "matrix/yearly.rs"]
mod yearly;

#[cfg(not(unix))]
fn main() {}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    let words: Vec<String> = std::env::args().skip(1).collect();
    let args = args::Args::parse(&words);
    let p = &args.profile;
    let machine = machine::machine(&args.data_root);
    println!(
        "matrix: {} profile, {} repeats, on {machine}",
        p.name, p.repeats
    );
    let run = Run {
        args: &args,
        machine: machine.clone(),
        started: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        began: Instant::now(),
    };
    let mut metrics = stats::Metrics::default();
    // Repeats interleave, every row once per round, so a slow minute on the
    // machine spreads over rows instead of landing on one. The report is written
    // after every row, so a failure hours in keeps what was measured.
    for round in 1..=p.repeats {
        println!("round {round} of {}", p.repeats);
        for row in p.rows.iter().filter(|r| args.runs(r.group)) {
            rows::run_row(&args.data_root, row, &mut metrics).await;
            run.save(&metrics);
        }
        if args.runs("history") {
            history::run_history(&args.data_root, &p.history, &mut metrics).await;
            run.save(&metrics);
        }
    }
    let (path, markdown) = run.save(&metrics);
    println!("{markdown}\nwrote {}.{{json,md}}", path.display());
    let Some(file) = &args.baseline else { return };
    let text = std::fs::read_to_string(file).unwrap();
    let base: serde_json::Value = serde_json::from_str(&text).unwrap();
    let verdict = baseline::compare(&metrics, p.name, &machine, &base, args.threshold);
    for note in &verdict.notes {
        println!("baseline note: {note}");
    }
    for regression in &verdict.regressions {
        println!("REGRESSION: {regression}");
    }
    println!("baseline: {} gated metrics compared", verdict.compared);
    if !verdict.passed() {
        eprintln!(
            "matrix: the regression check failed against {}",
            file.display()
        );
        std::process::exit(1);
    }
}

/// What every report of this run carries.
#[cfg(unix)]
struct Run<'a> {
    args: &'a args::Args,
    machine: serde_json::Value,
    started: u64,
    began: std::time::Instant,
}

#[cfg(unix)]
impl Run<'_> {
    /// Writes the JSON and Markdown reports of what has been measured so far.
    fn save(&self, metrics: &stats::Metrics) -> (std::path::PathBuf, String) {
        use serde_json::{Map, json};

        let p = &self.args.profile;
        let year = yearly::extrapolate(metrics);
        let mut context = Map::new();
        context.insert("profile".into(), json!(p.name));
        context.insert("repeats".into(), json!(p.repeats));
        context.insert("started_unix_s".into(), json!(self.started));
        context.insert("wall_s".into(), json!(self.began.elapsed().as_secs_f64()));
        context.insert("machine".into(), self.machine.clone());
        context.insert("commit".into(), machine::commit());
        let config = json!({"daemon_toml": daemon::CONFIG, "profile": self.args.describe()});
        context.insert("config".into(), config);
        let report = report::json(context, metrics, &year);
        let markdown = report::markdown(&report);
        std::fs::create_dir_all(&self.args.out).unwrap();
        let path = self.args.out.join(format!("matrix-{}", p.name));
        let text = serde_json::to_string_pretty(&report).unwrap();
        std::fs::write(path.with_extension("json"), text + "\n").unwrap();
        std::fs::write(path.with_extension("md"), &markdown).unwrap();
        (path, markdown)
    }
}
