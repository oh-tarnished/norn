//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Encryption's crates stay where encryption is.
//!
//! Only `norn-storage` may name the RustCrypto crates opt-in encryption at rest
//! uses (docs/reference/storage.md "Encryption"), and nothing beneath it may be
//! a cipher library with C or assembly in it. `deps` would catch most of those by
//! their build scripts; this names them, so a crate that vendors its objects
//! and has no build script still fails. `deny.toml` holds the same line for the
//! direct dependencies, for whoever runs only `just deny`.

use crate::metadata::{DepKind, Graph};
use crate::runtime_rs::closure_avoiding;
use crate::scan::Findings;

/// The member that may depend on [`CRYPTO`] directly.
pub const PERMITTED: &str = "norn-storage";

/// The crates only [`PERMITTED`] may name. Others reach them transitively:
/// `sha2`, `zeroize` and `getrandom` are in the daemon's graph regardless.
pub const CRYPTO: &[&str] = &["chacha20poly1305", "hkdf", "sha2", "zeroize", "getrandom"];

/// What must never be beneath [`PERMITTED`]: not pure Rust.
pub const IMPURE: &[&str] = &["ring", "aws-lc-rs", "aws-lc-sys", "openssl", "openssl-sys"];

/// Charges each other member naming a [`CRYPTO`] crate in what it ships, and any
/// [`IMPURE`] crate beneath [`PERMITTED`].
pub fn check_contained(graph: &Graph, findings: &mut Findings) {
    let name_of = |id: &str| graph.packages.get(id).map_or("", |p| p.name.as_str());
    for member in &graph.members {
        findings.examined += 1;
        let name = name_of(member);
        if name == PERMITTED {
            for reached in closure_avoiding(graph, name, &[]) {
                if IMPURE.contains(&reached.as_str()) {
                    let detail = format!("beneath {PERMITTED}: encryption is pure Rust");
                    findings.object(&reached, detail);
                }
            }
            continue;
        }
        for edge in graph.edges.get(member).into_iter().flatten() {
            let shipped = edge.kinds.iter().any(|k| *k != DepKind::Dev);
            let to = name_of(&edge.to);
            if shipped && CRYPTO.contains(&to) {
                let detail = format!("named by {name}; only {PERMITTED} may (xtask/src/crypto.rs)");
                findings.object(to, detail);
            }
        }
    }
}
