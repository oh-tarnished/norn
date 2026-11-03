//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Only the root has a README: a second one drifts from the first.

use std::collections::BTreeSet;

use crate::scan::Findings;

/// True for `README`, `readme.md`, `Readme.txt` and the like.
fn is_readme(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name);
    stem.eq_ignore_ascii_case("readme")
}

/// Fails on any README but `README.md` at the root, and on that one missing.
/// `examined` counts every path, and an empty listing fails.
#[must_use]
pub fn check(tracked: &BTreeSet<String>) -> Findings {
    let mut findings = Findings::new("docs readmes");
    findings.examined = tracked.len();
    if !tracked.is_empty() && !tracked.contains("README.md") {
        findings.object("README.md", "the root has no README.md");
    }
    for path in tracked {
        if is_readme(path) && path != "README.md" {
            findings.object(path.as_str(), "only the root may have a README");
        }
    }
    findings
}
