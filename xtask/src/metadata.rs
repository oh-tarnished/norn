//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The workspace graph, read from `cargo metadata`.
//!
//! Parsed into a small model so the graph gates are pure functions over it,
//! and their tests build graphs by hand instead of shelling out to cargo.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// How one package depends on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    /// Linked into the dependent's normal build.
    Normal,
    /// Used by the dependent's build script.
    Build,
    /// Used only by the dependent's tests, examples and benches.
    Dev,
}

/// One package in the graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    /// The package name, e.g. `norn-core`.
    pub name: String,
    /// The `links` key: a declaration that the package links a native
    /// library.
    pub links: Option<String>,
    /// Names of its build-dependencies, as declared.
    pub build_deps: Vec<String>,
    /// Its crate root: the library's `src_path`, else the first binary's.
    pub crate_root: Option<PathBuf>,
}

/// One resolved dependency edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The dependency's package id.
    pub to: String,
    /// Every way the dependent uses it.
    pub kinds: Vec<DepKind>,
}

/// The resolved workspace graph, keyed by package id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// Every package in the resolve.
    pub packages: BTreeMap<String, Package>,
    /// Each package's resolved dependencies.
    pub edges: BTreeMap<String, Vec<Edge>>,
    /// The workspace members' ids.
    pub members: Vec<String>,
}

/// Runs `cargo metadata` in `root` for every platform, and parses it.
///
/// # Errors
///
/// When cargo cannot run or its output is not the expected JSON.
pub fn load(root: &Path) -> Result<Graph, String> {
    load_for(root, &[])
}

/// As [`load`], but resolving only the dependencies that `platforms` (target
/// triples) build, all of them together; every platform when empty. A
/// dependency declared for another target, such as one only Haiku or wasm32
/// compiles, is not in the graph, because no build of Norn links it.
///
/// # Errors
///
/// As [`load`]; also when cargo does not know a triple.
pub fn load_for(root: &Path, platforms: &[&str]) -> Result<Graph, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let filters = platforms.iter().flat_map(|p| ["--filter-platform", p]);
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .args(filters)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let json: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    parse(&json)
}

/// Parses `cargo metadata --format-version 1` output.
///
/// # Errors
///
/// When the JSON lacks the resolve graph (`--no-deps` output) or a package
/// lacks an id.
pub fn parse(json: &Value) -> Result<Graph, String> {
    let mut graph = Graph::default();
    for p in array(json, "packages") {
        let id = text(p, "id").ok_or("a package without an id")?;
        let build_deps = array(p, "dependencies")
            .filter(|d| d.get("kind").and_then(Value::as_str) == Some("build"))
            .filter_map(|d| text(d, "name"))
            .collect();
        let package = Package {
            name: text(p, "name").unwrap_or_default(),
            links: text(p, "links"),
            build_deps,
            crate_root: crate_root(p),
        };
        graph.packages.insert(id, package);
    }
    graph.members = array(json, "workspace_members")
        .filter_map(|m| m.as_str().map(String::from))
        .collect();
    let nodes = json.pointer("/resolve/nodes").and_then(Value::as_array);
    for node in nodes.ok_or("cargo metadata output has no resolve graph")? {
        let id = text(node, "id").ok_or("a resolve node without an id")?;
        let edges = array(node, "deps")
            .filter_map(|d| {
                let kinds = array(d, "dep_kinds").map(|k| kind(k.get("kind"))).collect();
                Some(Edge {
                    to: text(d, "pkg")?,
                    kinds,
                })
            })
            .collect();
        graph.edges.insert(id, edges);
    }
    Ok(graph)
}

fn kind(value: Option<&Value>) -> DepKind {
    match value.and_then(Value::as_str) {
        Some("build") => DepKind::Build,
        Some("dev") => DepKind::Dev,
        _ => DepKind::Normal,
    }
}

fn crate_root(p: &Value) -> Option<PathBuf> {
    let targets: Vec<_> = array(p, "targets").collect();
    let is =
        |t: &&Value, k: &str| array(t, "kind").any(|x| x.as_str().is_some_and(|x| x.ends_with(k)));
    let root = targets
        .iter()
        .find(|t| is(t, "lib"))
        .or_else(|| targets.iter().find(|t| is(t, "bin")))?;
    text(root, "src_path").map(PathBuf::from)
}

fn array<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).into_iter().flatten()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}
