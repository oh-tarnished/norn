//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The harness's command line, after cargo's own `--bench`:
//!
//! ```text
//! --quick | --full          the profile (default: quick under `cargo bench`,
//!                           smoke under `cargo test --benches`)
//! --repeat N                repeats of every row and of the history
//! --only GROUP[,GROUP]      ingest, planes, history
//! --history-signals N       the history's signal count
//! --data-root DIR           where daemons keep their data: the disk measured
//! --out DIR                 where matrix-<profile>.{json,md} are written
//! --baseline FILE           compare with a report; fail on a regression
//! --threshold FACTOR        how much worse a gated median may be (default 3)
//! ```
//!
//! A relative path is taken from the workspace root, because cargo runs a bench
//! in its package's directory.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::baseline::THRESHOLD;
use crate::profile::{self, Profile};

pub struct Args {
    pub profile: Profile,
    pub groups: Vec<String>,
    pub data_root: PathBuf,
    pub out: PathBuf,
    pub baseline: Option<PathBuf>,
    pub threshold: f64,
}

const VALUED: [&str; 7] = [
    "--repeat",
    "--only",
    "--history-signals",
    "--data-root",
    "--out",
    "--baseline",
    "--threshold",
];
const FLAGS: [&str; 3] = ["--bench", "--quick", "--full"];

fn from_workspace(path: String) -> PathBuf {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    package.ancestors().nth(2).unwrap_or(package).join(path)
}

fn usage(why: &str) -> ! {
    eprintln!("matrix: {why}\nsee crates/server/benches/matrix/args.rs for the flags");
    std::process::exit(2)
}

impl Args {
    pub fn parse(words: &[String]) -> Self {
        let value = |flag: &str| {
            let at = words.iter().position(|w| w == flag)?;
            Some(words.get(at + 1).cloned().unwrap_or_else(|| usage(flag)))
        };
        let mut skip = false;
        for w in words {
            let known = FLAGS.contains(&w.as_str()) || VALUED.contains(&w.as_str());
            if !skip && w.starts_with("--") && !known {
                usage(&format!("unknown flag {w}"));
            }
            skip = VALUED.contains(&w.as_str());
        }
        let has = |flag: &str| words.iter().any(|w| w == flag);
        let mut profile = match (has("--full"), has("--quick") || has("--bench")) {
            (true, _) => profile::full(),
            (false, true) => profile::quick(),
            (false, false) => profile::smoke(),
        };
        let number =
            |flag: &str| value(flag).map(|v| v.parse::<usize>().unwrap_or_else(|_| usage(flag)));
        if let Some(n) = number("--repeat") {
            profile.repeats = n.max(1);
        }
        if let Some(n) = number("--history-signals") {
            profile.history.signals = n.max(1);
        }
        let groups = value("--only").map_or_else(
            || vec!["ingest".into(), "planes".into(), "history".into()],
            |v| v.split(',').map(str::to_owned).collect(),
        );
        let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let out = target
            .parent()
            .map_or(target.clone(), |t| t.join("bench-matrix"));
        let threshold = value("--threshold").map(|v| v.parse().unwrap_or_else(|_| usage(&v)));
        Self {
            profile,
            groups,
            data_root: value("--data-root").map_or_else(std::env::temp_dir, from_workspace),
            out: value("--out").map_or(out, from_workspace),
            baseline: value("--baseline").map(from_workspace),
            threshold: threshold.unwrap_or(THRESHOLD),
        }
    }

    pub fn runs(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
    }

    /// The profile as run, for the report: every scale and duration.
    pub fn describe(&self) -> Value {
        let p = &self.profile;
        let rows: Vec<Value> = p
            .rows
            .iter()
            .filter(|r| self.runs(r.group))
            .map(|r| {
                json!({"key": format!("{}.{}", r.group, r.case), "signals": r.signals,
                "offered_per_s": r.rate, "batch": r.batch, "seconds": r.secs,
                "ack_level": format!("{:?}", r.level), "plane": format!("{:?}", r.plane)})
            })
            .collect();
        let h = &p.history;
        json!({
            "name": p.name, "repeats": p.repeats, "groups": self.groups, "rows": rows,
            "history": {"signals": h.signals, "seconds": h.seconds, "windows": h.windows,
                "rounds": h.rounds, "replay_seconds": h.replay_seconds,
                "crash_seconds": h.crash_seconds},
        })
    }
}
