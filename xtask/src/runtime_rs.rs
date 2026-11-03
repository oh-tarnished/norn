//! The one exception to "no C" and the one place OpenTelemetry may appear:
//! runtime-rs, beneath the daemon.
//!
//! `norn-server` serves gRPC on its local socket or pipe, and MCP beside it,
//! through runtime-rs's `tpp-grpc` (`HybridServer`) and `tpp-agents`, rather than
//! assembling listeners, limits, drain and MCP's transports by hand. The owner
//! accepted on 2026-11-02 what they bring, which Norn would not choose itself:
//!
//! - **ring.** `tpp-grpc` builds tonic with `tls-ring`, so `ring` (C and
//!   assembly, `links = "ring_core_…"`) is in the graph. Norn configures no TLS:
//!   the daemon listens on a local endpoint. Cargo unifies tonic's and rustls's
//!   features across the workspace, so with the runtime present every crate using
//!   tonic appears to reach ring through `tokio-rustls` and `rustls`. What is
//!   excused is reaching ring *only* through those shared crates or the runtime,
//!   and only while the runtime is in the graph; a crate naming ring itself, or
//!   reaching it any other way, still fails.
//! - **OpenTelemetry.** `tpp-grpc` records transport spans and metrics through
//!   `opentelemetry`'s global providers. Norn never names an OpenTelemetry type.
//!   Accepted beneath `norn-server` (the `norn` binary) only.
//!
//! The engine crates ([`ENGINE`]) must reach none of it, not even through the
//! shared crates: they stay pure Rust whatever the daemon links. Upstream,
//! `tpp-grpc` without `tls-ring` would take ring out again.

use std::collections::{BTreeSet, VecDeque};

use crate::metadata::{DepKind, Graph};
use crate::scan::Findings;

/// The runtime-rs crates the exception is for.
pub const RUNTIME: &[&str] = &["tpp-grpc", "tpp-agents"];

/// The only member that may depend on [`RUNTIME`] and reach OpenTelemetry.
pub const PERMITTED: &[&str] = &["norn-server"];

/// The native library [`RUNTIME`] brings, by crate name.
pub const NATIVE: &[&str] = &["ring"];

/// The shared crates on which the runtime's features select ring.
pub const SHARED: &[&str] = &["rustls", "rustls-webpki", "tokio-rustls"];

/// The engine: pure Rust whatever the daemon links (SPEC §1).
pub const ENGINE: &[&str] = &[
    "norn-core",
    "norn-codec",
    "norn-storage",
    "norn-index",
    "norn-query",
    "norn-state",
    "norn-replay",
    "norn-runtime",
    "norn-metadata",
];

fn otel(name: &str) -> bool {
    name.starts_with("opentelemetry") || name.starts_with("tracing-opentelemetry")
}

/// The names of every package `member` reaches over normal edges, not passing
/// through a package named in `avoid`.
#[must_use]
pub fn closure_avoiding(graph: &Graph, member: &str, avoid: &[&str]) -> BTreeSet<String> {
    let name_of = |id: &str| graph.packages.get(id).map_or("", |p| p.name.as_str());
    let start = graph.members.iter().find(|id| name_of(id) == member);
    let mut queue: VecDeque<String> = start.cloned().into_iter().collect();
    let (mut seen, mut names) = (BTreeSet::new(), BTreeSet::new());
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        names.insert(name_of(&id).to_owned());
        for edge in graph.edges.get(&id).into_iter().flatten() {
            let to = name_of(&edge.to);
            if edge.kinds.contains(&DepKind::Normal) && !avoid.contains(&to) {
                queue.push_back(edge.to.clone());
            }
        }
    }
    names
}

/// Whether `package`, reached from `member`, is [`NATIVE`] and reached only
/// through [`SHARED`] or (for [`PERMITTED`]) [`RUNTIME`], in a graph holding the
/// runtime. Never for an [`ENGINE`] crate.
#[must_use]
pub fn excuses(graph: &Graph, member: &str, package: &str) -> bool {
    let present = graph
        .packages
        .values()
        .any(|p| RUNTIME.contains(&p.name.as_str()));
    if !NATIVE.contains(&package) || !present || ENGINE.contains(&member) {
        return false;
    }
    let mut avoid = SHARED.to_vec();
    if PERMITTED.contains(&member) {
        avoid.extend(RUNTIME);
    }
    !closure_avoiding(graph, member, &avoid).contains(package)
}

/// Charges each member outside [`PERMITTED`] that reaches [`RUNTIME`] or
/// OpenTelemetry, and each [`ENGINE`] crate that reaches those, ring, or rustls.
pub fn check_contained(graph: &Graph, findings: &mut Findings) {
    let name_of = |id: &str| graph.packages.get(id).map_or("", |p| p.name.as_str());
    for member in graph.members.iter().map(|id| name_of(id).to_owned()) {
        findings.examined += 1;
        if PERMITTED.contains(&member.as_str()) {
            continue;
        }
        let engine = ENGINE.contains(&member.as_str());
        for name in closure_avoiding(graph, &member, &[]) {
            let foreign = RUNTIME.contains(&name.as_str()) || otel(&name);
            let tls = NATIVE.contains(&name.as_str()) || SHARED.contains(&name.as_str());
            if foreign || (engine && tls) {
                let why = if engine {
                    "an engine crate stays pure Rust"
                } else {
                    "only norn-server may"
                };
                findings.object(
                    &name,
                    format!("reached from {member}; {why} (xtask/src/runtime_rs.rs)"),
                );
            }
        }
    }
}
