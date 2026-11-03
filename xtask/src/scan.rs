//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Finding the tree, walking it, and refusing to pass when it is empty.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directory names no gate descends into. `generated` is exempt by rule:
/// generated output is rewritten wholesale and its length is not a design
/// decision. The rest are machinery, not source.
pub const SKIP_DIRS: &[&str] = &["target", ".git", "generated", "node_modules"];

/// The workspace root, found by walking up from this crate's manifest.
///
/// # Errors
///
/// When no ancestor holds a `Cargo.toml` declaring `[workspace]`. The gates
/// fail rather than fall back to the current directory: a gate that cannot
/// see the tree must not report a pass.
pub fn repo_root() -> Result<PathBuf, String> {
    let start = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in start.ancestors() {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml"));
        if manifest.is_ok_and(|text| text.contains("[workspace]")) {
            return Ok(dir.to_path_buf());
        }
    }
    Err(format!("no workspace manifest above {}", start.display()))
}

/// Every file under `root`, relative to it and sorted, skipping
/// [`SKIP_DIRS`] by whole path component.
#[must_use]
pub fn files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(root, root, &mut found);
    found.sort();
    found
}

fn collect(root: &Path, dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            // A whole-name test: a prefix test for ".git" would skip ".github".
            if !SKIP_DIRS.iter().any(|skip| name == *skip) {
                collect(root, &path, found);
            }
        } else if let Ok(rel) = path.strip_prefix(root) {
            found.push(rel.to_path_buf());
        }
    }
}

/// The repository's files: tracked, plus untracked ones git does not ignore,
/// so a new document is checked before its commit. Forward slashes, relative
/// to `root`.
///
/// # Errors
///
/// When git cannot run or `root` is no repository: a gate that cannot list
/// the tree must not pass.
pub fn tracked(root: &Path) -> Result<BTreeSet<String>, String> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

/// A file's path, relative to the root, and its contents.
pub type SourceFile = (PathBuf, String);

/// A subject and the detail of what is wrong with it.
pub type Objection = (String, String);

/// Reads each of `paths` (relative to `dir`), returning the readable files
/// and, separately, `(path, error)` for each unreadable one. An unreadable
/// file must become an objection, never a silently skipped file.
#[must_use]
pub fn read(dir: &Path, paths: Vec<PathBuf>) -> (Vec<SourceFile>, Vec<Objection>) {
    let (mut ok, mut failed) = (Vec::new(), Vec::new());
    for p in paths {
        match std::fs::read_to_string(dir.join(&p)) {
            Ok(text) => ok.push((p, text)),
            Err(e) => failed.push((display(&p), format!("unreadable: {e}"))),
        }
    }
    (ok, failed)
}

/// One objection a gate raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file or crate objected to.
    pub subject: String,
    /// What is wrong, for a human.
    pub detail: String,
}

/// What a gate examined and what it objected to.
///
/// The count matters as much as the objections: a gate that examined nothing
/// has not passed, it has failed to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Findings {
    /// The gate's name, as `xtask <name>` runs it.
    pub gate: &'static str,
    /// How many things the gate inspected.
    pub examined: usize,
    /// Its objections; empty on a pass.
    pub problems: Vec<Problem>,
}

impl Findings {
    /// An empty report for `gate`.
    #[must_use]
    pub fn new(gate: &'static str) -> Self {
        Self {
            gate,
            examined: 0,
            problems: Vec::new(),
        }
    }

    /// Counts and records each `(subject, detail)` as an objection.
    pub fn unreadable(&mut self, failed: Vec<Objection>) {
        for (subject, detail) in failed {
            self.examined += 1;
            self.object(subject, detail);
        }
    }

    /// Records an objection.
    pub fn object(&mut self, subject: impl Into<String>, detail: impl Into<String>) {
        self.problems.push(Problem {
            subject: subject.into(),
            detail: detail.into(),
        });
    }

    /// Pass or fail, with a report either way.
    ///
    /// # Errors
    ///
    /// When the gate examined nothing, or raised any objection.
    pub fn verdict(&self) -> Result<String, String> {
        if self.examined == 0 {
            return Err(format!(
                "{}: examined nothing, which is a failure and not a pass",
                self.gate
            ));
        }
        if self.problems.is_empty() {
            return Ok(format!("{}: ok ({} examined)", self.gate, self.examined));
        }
        let mut report = format!("{}: {} problem(s)", self.gate, self.problems.len());
        for p in &self.problems {
            report.push_str(&format!("\n  {}: {}", p.subject, p.detail));
        }
        Err(report)
    }
}

/// `path` with forward slashes, so reports and rules read the same on
/// Windows.
#[must_use]
pub fn display(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
