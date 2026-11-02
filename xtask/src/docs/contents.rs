//! The README's table of contents mirrors `docs/`, both ways.

use std::collections::BTreeSet;

use super::markdown;
use crate::scan::Findings;

/// The heading that opens the table of contents.
pub const HEADING: &str = "Contents";

/// The `docs/` files linked from `readme`'s contents section, or `None` when
/// it has no such section. Ends at the next heading of level two or above.
fn entries(readme: &str) -> Option<BTreeSet<String>> {
    let mut inside = false;
    let mut section = String::new();
    let mut found = false;
    for line in markdown::prose(readme) {
        if line.strip_prefix("## ").map(str::trim) == Some(HEADING) {
            inside = true;
            found = true;
            continue;
        }
        if inside && (line.starts_with("# ") || line.starts_with("## ")) {
            inside = false;
        }
        if inside {
            section.push_str(&line);
            section.push('\n');
        }
    }
    found.then(|| {
        markdown::links(&section)
            .into_iter()
            .map(|l| l.target)
            .map(|t| t.split('#').next().unwrap_or("").to_owned())
            .map(|t| t.strip_prefix("./").unwrap_or(&t).to_owned())
            .filter(|t| t.starts_with("docs/"))
            .collect()
    })
}

/// Compares `readme`'s contents with `tracked`, the repository's paths. `None`
/// for a missing README. `examined` counts the distinct paths on either side,
/// so an empty `docs/` with an empty table fails.
#[must_use]
pub fn check(readme: Option<&str>, tracked: &BTreeSet<String>) -> Findings {
    let mut findings = Findings::new("docs contents");
    let docs: BTreeSet<&String> = tracked.iter().filter(|p| p.starts_with("docs/")).collect();
    let Some(listed) = readme.and_then(entries) else {
        findings.examined += docs.len();
        findings.object("README.md", format!("has no `## {HEADING}` section"));
        return findings;
    };
    let listed: BTreeSet<&String> = listed.iter().collect();
    findings.examined = docs.union(&listed).count();
    for path in docs.difference(&listed) {
        findings.object("README.md", format!("contents omit `{path}`"));
    }
    for path in listed.difference(&docs) {
        findings.object("README.md", format!("contents list `{path}`, not in docs/"));
    }
    findings
}
