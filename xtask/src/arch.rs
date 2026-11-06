//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `xtask arch`: dependencies between Norn crates only point inward.
//!
//! [`LAYERS`] is the authority (mirrored in docs/reference/architecture.md). Each entry
//! names a package and the workspace packages it may depend on, through
//! normal or build edges; dev-dependencies are exempt, since a test may
//! assemble layers it does not belong to. A workspace member missing from
//! the table is an error: adding a crate means deciding where it sits.
//! Entries for crates that do not exist yet are the plan (docs/project/plan.md) and cost
//! nothing.

use crate::metadata::{DepKind, Graph};
use crate::scan::Findings;

/// Package name → the workspace packages it may import.
pub const LAYERS: &[(&str, &[&str])] = &[
    // The vocabulary. Imports nothing.
    ("norn-core", &[]),
    // Leaves over the core: wire schema, encodings, metadata, byte transport.
    ("norn-protocol", &["norn-core"]),
    ("norn-codec", &["norn-core"]),
    ("norn-metadata", &["norn-core"]),
    ("norn-transport", &["norn-core"]),
    // Connections and framing over a transport.
    (
        "norn-ipc",
        &["norn-core", "norn-protocol", "norn-transport"],
    ),
    // Implements norn-ipc's DataPlane and Attach seams, so it sits above it;
    // norn-ipc never imports it back, and falls back to gRPC without it.
    (
        "norn-shared-memory",
        &["norn-core", "norn-transport", "norn-ipc"],
    ),
    // The data plane, innermost first.
    ("norn-storage", &["norn-core", "norn-codec"]),
    ("norn-index", &["norn-core", "norn-storage"]),
    (
        "norn-query",
        &["norn-core", "norn-codec", "norn-storage", "norn-index"],
    ),
    (
        "norn-state",
        &["norn-core", "norn-storage", "norn-index", "norn-query"],
    ),
    (
        "norn-replay",
        &[
            "norn-core",
            "norn-storage",
            "norn-index",
            "norn-query",
            "norn-state",
        ],
    ),
    // A database as a whole: its version, backups, restore, migration.
    (
        "norn-database",
        &["norn-core", "norn-storage", "norn-metadata"],
    ),
    // The daemon assembles the data plane...
    (
        "norn-runtime",
        &[
            "norn-core",
            "norn-codec",
            "norn-metadata",
            "norn-storage",
            "norn-database",
            "norn-index",
            "norn-query",
            "norn-state",
            "norn-replay",
        ],
    ),
    // ...and serves it.
    (
        "norn-server",
        &[
            "norn-core",
            "norn-protocol",
            "norn-transport",
            "norn-ipc",
            "norn-shared-memory",
            "norn-metadata",
            "norn-database",
            "norn-runtime",
        ],
    ),
    // Clients reach Norn only through the protocol (SPEC §31, §33, §47).
    // nornctl also drives the export and import formats, each a client library
    // of the public API like itself, and opts into the shared-memory data plane
    // (`--shared-memory`), which is a client of norn-ipc's seam. Its offline
    // commands, `restore` and `migrate`, run with no daemon to be a client of, so
    // they call norn-database, and that is all of the data plane it may name.
    (
        "norn-cli",
        &[
            "norn-core",
            "norn-protocol",
            "norn-transport",
            "norn-ipc",
            "norn-shared-memory",
            "norn-adapter",
            "norn-adapter-csv",
            "norn-adapter-parquet",
            "norn-adapter-mcap",
            "norn-database",
        ],
    ),
    // Adapters are clients too (SPEC §29, §47): the contract speaks the protocol,
    // and a format knows only the contract and the protocol's types. The server
    // cannot import any of them, which is why ExportStream stays UNIMPLEMENTED
    // (docs/reference/adapters.md). The testkit is a dev-dependency only.
    (
        "norn-adapter",
        &["norn-core", "norn-protocol", "norn-transport", "norn-ipc"],
    ),
    ("norn-adapter-csv", &["norn-adapter", "norn-protocol"]),
    ("norn-adapter-parquet", &["norn-adapter", "norn-protocol"]),
    ("norn-adapter-mcap", &["norn-adapter", "norn-protocol"]),
    ("norn-adapter-testkit", &["norn-adapter", "norn-protocol"]),
    // A live adapter's delivery into Norn, for device-protocol adapters in their
    // own repositories (SPEC §30, §47). A client crate like the rest: nothing in
    // this table may import it, least of all the engine or the daemon. Shared
    // memory is behind its opt-in feature, as nornctl's is behind a flag.
    (
        "norn-sink",
        &[
            "norn-protocol",
            "norn-transport",
            "norn-ipc",
            "norn-shared-memory",
        ],
    ),
    // Repository tooling imports no Norn crate.
    ("xtask", &[]),
];

/// Checks every member of `graph` against `layers`.
#[must_use]
pub fn check(graph: &Graph, layers: &[(&str, &[&str])]) -> Findings {
    let mut findings = Findings::new("arch");
    let name_of = |id: &str| graph.packages.get(id).map(|p| p.name.as_str());
    for member in &graph.members {
        findings.examined += 1;
        let Some(name) = name_of(member) else {
            findings.object(member, "member missing from cargo metadata");
            continue;
        };
        let Some((_, allowed)) = layers.iter().find(|(n, _)| *n == name) else {
            findings.object(name, "has no entry in the layer table (xtask/src/arch.rs)");
            continue;
        };
        for edge in graph.edges.get(member).into_iter().flatten() {
            if !graph.members.contains(&edge.to) {
                continue;
            }
            let shipped = edge.kinds.iter().any(|k| *k != DepKind::Dev);
            let dep = name_of(&edge.to).unwrap_or("?");
            if shipped && !allowed.contains(&dep) {
                findings.object(
                    name,
                    format!("may not depend on {dep}; dependencies point inward"),
                );
            }
        }
    }
    findings
}
