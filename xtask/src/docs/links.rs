//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Every relative link and image in every Markdown file resolves.
//!
//! Resolved against the tracked set, not the disk: a link to an ignored file
//! works here and breaks on GitHub, and a tracked set is case-sensitive on a
//! case-insensitive disk, as GitHub is.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{headings, markdown};
use crate::scan::{self, Findings, SourceFile};

/// True for a target this gate cannot check: `https:`, `mailto:`, `//host`.
fn is_external(target: &str) -> bool {
    if target.starts_with("//") {
        return true;
    }
    target.split_once(':').is_some_and(|(scheme, _)| {
        scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// `target` joined to `dir` with `.` and `..` folded; `None` above the root.
/// A leading `/` means the repository root, as it does on GitHub.
fn resolve(dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn exists(path: &str, tracked: &BTreeSet<String>) -> bool {
    if path.is_empty() || tracked.contains(path) {
        return true;
    }
    let dir = format!("{path}/");
    tracked
        .range(dir.clone()..)
        .next()
        .is_some_and(|p| p.starts_with(&dir))
}

/// Checks every link in `documents` against `tracked`, the repository's file
/// paths with forward slashes. `examined` counts links, so documents holding
/// none fail rather than pass.
#[must_use]
pub fn check(documents: &[SourceFile], tracked: &BTreeSet<String>) -> Findings {
    let mut findings = Findings::new("docs links");
    let anchors: BTreeMap<String, BTreeSet<String>> = documents
        .iter()
        .map(|(path, text)| (scan::display(path), headings::anchors(text)))
        .collect();
    for (path, text) in documents {
        let name = scan::display(path);
        let dir = Path::new(&name)
            .parent()
            .map(scan::display)
            .unwrap_or_default();
        for link in markdown::links(text) {
            if is_external(&link.target) {
                continue;
            }
            findings.examined += 1;
            let subject = format!("{name}:{}", link.line);
            let (file, anchor) = match link.target.split_once('#') {
                Some((file, anchor)) => (file, Some(anchor)),
                None => (link.target.as_str(), None),
            };
            let file = file.replace("%20", " ");
            let resolved = if file.is_empty() {
                Some(name.clone())
            } else {
                resolve(&dir, &file)
            };
            let Some(resolved) = resolved.filter(|r| exists(r, tracked)) else {
                findings.object(subject, format!("`{}` does not resolve", link.target));
                continue;
            };
            let Some(anchor) = anchor.filter(|_| resolved.ends_with(".md")) else {
                continue;
            };
            if !anchors.get(&resolved).is_some_and(|a| a.contains(anchor)) {
                findings.object(subject, format!("`{resolved}` has no anchor `#{anchor}`"));
            }
        }
    }
    findings
}
