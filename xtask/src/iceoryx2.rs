//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The second exception to "no C": iceoryx2's platform crates, beneath the
//! shared-memory data plane only.
//!
//! The owner approved iceoryx2 on 2026-11-03 for `norn-shared-memory`. Two of
//! its crates name native-build helpers that never run on a target Norn builds,
//! audited at 0.10.0 (pinned exactly in `norn-shared-memory`'s Cargo.toml):
//!
//! - `iceoryx2-pal-posix`: `build.rs` runs `bindgen`, and through it `cc`, only
//!   under `#[cfg(target_os = "nto")]`, a QNX build host. Every other target
//!   binds the OS through the `libc` and `windows-sys` crates, pure Rust.
//! - `iceoryx2-pal-os-api`: `build = false`; it has no build script at all.
//!
//! So what is excused is narrow: one helper of one audited package, reached from
//! one of [`PERMITTED`]. A `links` key is never excused, nor any other package,
//! nor any helper reached from another member; and [`check_contained`] charges
//! any other member that reaches iceoryx2 at all, so it cannot spread beneath the
//! engine or a client library by accident. Re-read both build scripts before
//! moving the pin, and say why in the commit.

use crate::metadata::Graph;
use crate::runtime_rs::{ENGINE, closure_avoiding};
use crate::scan::Findings;

/// `(package, helper, audit)`: a native-build helper an audited package names
/// but never runs on [`crate::deps::PLATFORMS`].
pub const AUDITED: &[(&str, &str, &str)] = &[
    (
        "iceoryx2-pal-posix",
        "cc",
        "0.10.0: build.rs compiles C only under cfg(target_os = \"nto\") (QNX hosts)",
    ),
    (
        "iceoryx2-pal-posix",
        "bindgen",
        "0.10.0: build.rs runs bindgen only under cfg(target_os = \"nto\") (QNX hosts)",
    ),
    (
        "iceoryx2-pal-os-api",
        "cc",
        "0.10.0: `build = false`; it has no build script to run a helper",
    ),
    (
        "iceoryx2-pal-os-api",
        "bindgen",
        "0.10.0: `build = false`; it has no build script to run a helper",
    ),
];

/// The members that may reach iceoryx2: the data plane, the daemon that offers
/// it, and the client that opts into it (`nornctl --shared-memory`).
pub const PERMITTED: &[&str] = &["norn-shared-memory", "norn-server", "norn-cli"];

fn iceoryx2(name: &str) -> bool {
    name.starts_with("iceoryx2")
}

/// Whether `helper`, a build-dependency of `package` reached from `member`, is
/// one [`AUDITED`] never runs, and `member` is [`PERMITTED`] to reach it. Never
/// for an [`ENGINE`] crate.
#[must_use]
pub fn excuses(member: &str, package: &str, helper: &str) -> bool {
    PERMITTED.contains(&member)
        && !ENGINE.contains(&member)
        && AUDITED
            .iter()
            .any(|(p, h, _)| *p == package && *h == helper)
}

/// Charges each member outside [`PERMITTED`] that reaches any iceoryx2 crate.
pub fn check_contained(graph: &Graph, findings: &mut Findings) {
    let name_of = |id: &str| graph.packages.get(id).map_or("", |p| p.name.as_str());
    for member in graph.members.iter().map(|id| name_of(id).to_owned()) {
        findings.examined += 1;
        if PERMITTED.contains(&member.as_str()) {
            continue;
        }
        for name in closure_avoiding(graph, &member, &[]) {
            if iceoryx2(&name) {
                findings.object(
                    &name,
                    format!(
                        "reached from {member}; only {} may (xtask/src/iceoryx2.rs)",
                        PERMITTED.join(", ")
                    ),
                );
            }
        }
    }
}
