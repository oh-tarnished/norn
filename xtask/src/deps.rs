//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `xtask deps`: nothing a Norn crate links reaches C (SPEC §1).
//!
//! Walks each workspace member's normal dependency graph (build and dev edges
//! excluded: they never ship) and objects to any package that declares a
//! `links` key or build-depends on a native-build helper. A crate may opt out
//! only by appearing in [`ALLOW_C`] with its reason. The single exception for
//! what runtime-rs brings beneath the daemon, and the rule that keeps it
//! there, are [`crate::runtime_rs`]'s.

use std::collections::{BTreeSet, VecDeque};

use crate::metadata::{DepKind, Graph};
use crate::scan::Findings;

/// Build-dependencies that mean "compiles or locates native code".
pub const NATIVE_BUILD: &[&str] = &["cc", "cmake", "pkg-config", "vcpkg", "bindgen"];

/// The targets Norn builds: the release matrix (`.github/release/plan.sh`)
/// and CI's cross-compiles.
///
/// Why the gate is platform-scoped: "no C" is a promise about what Norn
/// links, and a dependency only another target compiles is linked by no
/// build of Norn. Without the scope, `iana-time-zone-haiku` (under `chrono`,
/// reached through `arrow` and `schemars`), whose build script runs `cc` on
/// Haiku alone, and `wasm-bindgen-shared`, whose `links` key is wasm32's and
/// names no C library, would fail a gate about binaries that cannot exist.
/// Only `deps` is scoped; `arch` and `docs` still see every platform. Adding
/// a target here widens the gate, never narrows it, and
/// `xtask/tests/platform_gates.rs` fails if a shipped target is missing.
pub const PLATFORMS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "armv7-unknown-linux-gnueabihf",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

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
            // Excused for this member only: another may reach it otherwise.
            if crate::runtime_rs::excuses(graph, name, &p.name) {
                findings.examined += 1;
                continue;
            }
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
    crate::runtime_rs::check_contained(graph, &mut findings);
    findings
}
