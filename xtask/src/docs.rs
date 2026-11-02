//! `xtask docs`: the documentation's shape.
//!
//! Every crate root opens with a `//!` layer contract: what the crate owns,
//! must not do and may import is the first thing a reader meets. Checks the
//! library root, or the binary's for a binary-only crate. Beside that:
//! [`links`] resolve, the README's [`contents`] mirror `docs/`, and only the
//! root has a README ([`readmes`]).

pub mod contents;
pub mod headings;
pub mod links;
pub mod markdown;
pub mod readmes;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::metadata::Graph;
use crate::scan::{self, Findings};

/// Checks `(crate, root path, contents)` triples; pure, so tests need no
/// files.
#[must_use]
pub fn check(roots: &[(String, PathBuf, String)]) -> Findings {
    let mut findings = Findings::new("docs");
    for (name, path, text) in roots {
        findings.examined += 1;
        let first = text.lines().map(str::trim).find(|l| !l.is_empty());
        if !first.is_some_and(|l| l.starts_with("//!")) {
            let detail = format!(
                "{} must open with a `//!` layer contract",
                scan::display(path)
            );
            findings.object(name, detail);
        }
    }
    findings
}

/// One verdict from several checks. A check that examined nothing is an
/// objection here, or the others' counts would hide it.
#[must_use]
pub fn combine(checks: Vec<Findings>) -> Findings {
    let mut all = Findings::new("docs");
    for check in checks {
        if check.examined == 0 {
            all.object(check.gate, "examined nothing, which is a failure");
        }
        all.examined += check.examined;
        all.problems.extend(check.problems);
    }
    all
}

/// The checks over the repository's Markdown; `tracked` is its file list.
fn tree_checks(root: &Path, tracked: &BTreeSet<String>) -> Vec<Findings> {
    let markdown: Vec<PathBuf> = tracked
        .iter()
        .filter(|p| p.ends_with(".md"))
        .map(PathBuf::from)
        .collect();
    let (documents, failed) = scan::read(root, markdown);
    let mut links = links::check(&documents, tracked);
    links.unreadable(failed);
    let readme = documents
        .iter()
        .find(|(path, _)| path == Path::new("README.md"))
        .map(|(_, text)| text.as_str());
    vec![
        links,
        contents::check(readme, tracked),
        readmes::check(tracked),
    ]
}

/// Runs the gate over every workspace member's crate root, then the tracked
/// Markdown. A file list git cannot give is an objection, not a skipped check.
#[must_use]
pub fn run(root: &Path, graph: &Graph) -> Findings {
    let mut roots = Vec::new();
    let mut unreadable = Vec::new();
    for member in &graph.members {
        let Some(p) = graph.packages.get(member) else {
            continue;
        };
        let Some(path) = &p.crate_root else {
            unreadable.push((p.name.clone(), "has no library or binary target".to_owned()));
            continue;
        };
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        match std::fs::read_to_string(path) {
            Ok(text) => roots.push((p.name.clone(), rel, text)),
            Err(e) => unreadable.push((p.name.clone(), format!("cannot read crate root: {e}"))),
        }
    }
    let mut findings = check(&roots);
    for (name, detail) in unreadable {
        findings.examined += 1;
        findings.object(name, detail);
    }
    let mut checks = vec![findings];
    match scan::tracked(root) {
        Ok(tracked) => checks.extend(tree_checks(root, &tracked)),
        Err(detail) => {
            let mut failed = Findings::new("docs tree");
            failed.examined += 1;
            failed.object("git ls-files", detail);
            checks.push(failed);
        }
    }
    combine(checks)
}
