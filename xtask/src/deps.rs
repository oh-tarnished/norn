//! `xtask deps`: nothing a Norn crate links reaches C (SPEC §1).
//!
//! Walks each workspace member's normal dependency graph (build and dev edges
//! excluded: they never ship) and objects to any package that declares a
//! `links` key or build-depends on a native-build helper. A crate may opt out
//! only by appearing in [`ALLOW_C`] with its reason.

use std::collections::{BTreeSet, VecDeque};

use crate::metadata::{DepKind, Graph};
use crate::scan::Findings;

/// Build-dependencies that mean "compiles or locates native code".
pub const NATIVE_BUILD: &[&str] = &["cc", "cmake", "pkg-config", "vcpkg", "bindgen"];

/// Workspace members whose graphs may reach C, with the reason. Empty: no
/// Norn crate needs C. Adding one is an architecture decision; record it in
/// docs/reference/architecture.md and the commit.
pub const ALLOW_C: &[(&str, &str)] = &[];

/// Checks every member of `graph` not in `allow` (member names).
#[must_use]
pub fn check(graph: &Graph, allow: &[(&str, &str)]) -> Findings {
    let mut findings = Findings::new("deps");
    let mut seen = BTreeSet::new();
    for member in &graph.members {
        let name = graph.packages.get(member).map_or("?", |p| p.name.as_str());
        if allow.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let mut queue = VecDeque::from([member.clone()]);
        let mut reached = BTreeSet::new();
        while let Some(id) = queue.pop_front() {
            if !reached.insert(id.clone()) {
                continue;
            }
            for edge in graph.edges.get(&id).into_iter().flatten() {
                if edge.kinds.contains(&DepKind::Normal) {
                    queue.push_back(edge.to.clone());
                }
            }
        }
        for id in reached {
            let Some(p) = graph.packages.get(&id) else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            findings.examined += 1;
            if let Some(lib) = &p.links {
                findings.object(
                    &p.name,
                    format!("declares `links = \"{lib}\"` (reached from {name})"),
                );
            }
            for helper in p
                .build_deps
                .iter()
                .filter(|d| NATIVE_BUILD.contains(&d.as_str()))
            {
                findings.object(
                    &p.name,
                    format!("build-depends on `{helper}` (reached from {name})"),
                );
            }
        }
    }
    findings
}
