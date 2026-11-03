//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `deps` examines what Norn's own targets link: a dependency only another
//! target compiles is out of it, and one a shipped target compiles is not.

use std::collections::BTreeSet;

use xtask::{deps, metadata, scan};

fn names(graph: &metadata::Graph) -> BTreeSet<&str> {
    graph.packages.values().map(|p| p.name.as_str()).collect()
}

#[test]
fn a_dependency_only_another_platform_compiles_is_left_out() {
    let root = scan::repo_root().unwrap();
    let everything = metadata::load(&root).unwrap();
    let shipped = metadata::load_for(&root, deps::PLATFORMS).unwrap();
    assert!(names(&everything).contains("iana-time-zone-haiku"));
    assert!(!names(&shipped).contains("iana-time-zone-haiku"));
    // What every target links survives the filter: it removes, never empties.
    assert!(
        names(&shipped).contains("tokio"),
        "the filter removed what ships"
    );
}

#[test]
fn a_dependency_only_windows_compiles_is_still_examined() {
    let root = scan::repo_root().unwrap();
    let shipped = metadata::load_for(&root, deps::PLATFORMS).unwrap();
    assert!(names(&shipped).contains("windows-sys"));
    let unix_only = metadata::load_for(&root, &["x86_64-unknown-linux-gnu"]).unwrap();
    assert!(
        !names(&unix_only).contains("windows-sys"),
        "the filter must bite"
    );
}

#[test]
fn every_release_and_cross_target_is_examined() {
    let root = scan::repo_root().unwrap();
    let plan = std::fs::read_to_string(root.join(".github/release/plan.sh")).unwrap();
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
    let mut found = 0;
    for triple in deps::PLATFORMS {
        found += usize::from(plan.contains(triple) || ci.contains(triple));
    }
    assert_eq!(
        found,
        deps::PLATFORMS.len(),
        "a listed target is built nowhere"
    );
    for line in plan.lines().chain(ci.lines()) {
        for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
            let triple = word.matches('-').count() >= 2 && word.contains("unknown")
                || word.ends_with("-apple-darwin")
                || word.ends_with("-windows-msvc");
            if triple {
                assert!(
                    deps::PLATFORMS.contains(&word),
                    "{word} ships but is unexamined"
                );
            }
        }
    }
}
