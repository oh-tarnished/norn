//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's repository gates.
//!
//! # Layer contract
//!
//! - **Owns**: the checks CI runs beyond the compiler: `size` (files under
//!   200 lines), `deps` (no C in any shipped graph), `arch` (dependencies
//!   point inward, every crate placed in the layer table), `docs` (crate
//!   roots open with a `//!` layer contract, Markdown links and anchors
//!   resolve, the README's contents mirror `docs/`, only the root has a
//!   README) and `purity` (no I/O or clock in the core).
//! - **Must not**: pass when it examined nothing. Every gate reports a count,
//!   and [`scan::Findings::verdict`] fails on zero; every gate has a test in
//!   `xtask/tests/` proving it bites.
//! - **May import**: no Norn crate; `serde_json` to read `cargo metadata`.
//! - **Spans**: none; this is tooling, not the engine.
//!
//! Concurrency: single-threaded, run once per invocation.

pub mod arch;
pub mod crypto;
pub mod deps;
pub mod docs;
pub mod iceoryx2;
pub mod metadata;
pub mod purity;
pub mod runtime_rs;
pub mod scan;
pub mod size;

use std::path::Path;

use scan::Findings;

/// Every gate's name, in the order `gates` runs them.
pub const GATES: &[&str] = &["size", "purity", "docs", "arch", "deps"];

/// Runs the named gate against the tree at `root`.
///
/// # Errors
///
/// For an unknown gate name, or when `cargo metadata` cannot be read.
pub fn run(gate: &str, root: &Path) -> Result<Findings, String> {
    match gate {
        "size" => Ok(size::run(root)),
        "purity" => Ok(purity::run(root)),
        "docs" => Ok(docs::run(root, &metadata::load(root)?)),
        "arch" => Ok(arch::check(&metadata::load(root)?, arch::LAYERS)),
        "deps" => {
            let graph = metadata::load_for(root, deps::PLATFORMS)?;
            Ok(deps::check(&graph, deps::ALLOW_C))
        }
        other => Err(format!(
            "unknown gate `{other}`; known: gates, {}",
            GATES.join(", ")
        )),
    }
}
