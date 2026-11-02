//! `xtask size`: every source file stays under 200 lines (SPEC §2).

use std::path::{Path, PathBuf};

use crate::scan::{self, Findings};

/// The first line count that fails. SPEC §2 says "under 200 lines".
pub const LIMIT: usize = 200;

/// The extensions the limit applies to. Markdown is documentation, which
/// SPEC §2 exempts.
pub const EXTENSIONS: &[&str] = &["rs", "proto", "py", "sh"];

/// Whether `path` (relative to the root) is subject to the limit. Paths under
/// a `generated` directory are exempt.
#[must_use]
pub fn applies(path: &Path) -> bool {
    let measured = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e));
    measured && !path.components().any(|c| c.as_os_str() == "generated")
}

/// Checks `(path, contents)` pairs; pure, so tests need no files.
#[must_use]
pub fn check(files: &[(PathBuf, String)]) -> Findings {
    let mut findings = Findings::new("size");
    for (path, text) in files.iter().filter(|(p, _)| applies(p)) {
        findings.examined += 1;
        let lines = text.lines().count();
        if lines >= LIMIT {
            let detail = format!("{lines} lines; split it along a seam (limit: under {LIMIT})");
            findings.object(scan::display(path), detail);
        }
    }
    findings
}

/// Runs the gate over the tree at `root`.
#[must_use]
pub fn run(root: &Path) -> Findings {
    let paths = scan::files(root)
        .into_iter()
        .filter(|p| applies(p))
        .collect();
    let (files, failed) = scan::read(root, paths);
    let mut findings = check(&files);
    findings.unreadable(failed);
    findings
}
