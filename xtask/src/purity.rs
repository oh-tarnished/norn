//! `xtask purity`: the core performs no I/O, reads no clock and spawns
//! nothing (SPEC §28: time is injected; the core is pure data).

use std::path::{Path, PathBuf};

use crate::scan::{self, Findings};

/// The directory whose sources must stay pure.
pub const PURE_DIR: &str = "crates/core/src";

/// Paths that mean I/O, a clock, a thread, a process or the environment.
pub const FORBIDDEN: &[&str] = &[
    "std::fs",
    "std::net",
    "std::io",
    "std::env",
    "std::process",
    "std::thread",
    "SystemTime",
    "Instant",
];

/// Checks `(path, contents)` pairs; pure, so tests need no files. Comment
/// lines are skipped, so documentation may name what the code must not do.
#[must_use]
pub fn check(files: &[(PathBuf, String)]) -> Findings {
    let mut findings = Findings::new("purity");
    for (path, text) in files {
        findings.examined += 1;
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for token in FORBIDDEN.iter().filter(|t| line.contains(**t)) {
                let subject = format!("{}:{}", scan::display(path), n + 1);
                findings.object(subject, format!("uses `{token}`; the core is pure"));
            }
        }
    }
    findings
}

/// Runs the gate over the `.rs` files under [`PURE_DIR`].
#[must_use]
pub fn run(root: &Path) -> Findings {
    let paths = scan::files(&root.join(PURE_DIR))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .map(|p| Path::new(PURE_DIR).join(p))
        .collect();
    let (files, failed) = scan::read(root, paths);
    let mut findings = check(&files);
    findings.unreadable(failed);
    findings
}
