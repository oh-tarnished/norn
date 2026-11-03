//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The iceoryx2 exception to `deps` is as narrow as xtask/src/iceoryx2.rs says.

mod common;

use common::graph;
use xtask::deps;
use xtask::metadata::DepKind::Normal;
use xtask::metadata::Graph;

/// `member` reaching `package`, which build-depends on `helpers`.
fn reaching(member: &str, package: &str, helpers: &[&str]) -> Graph {
    let mut g = graph(&[member], &[(member, package, Normal)]);
    if let Some(p) = g.packages.get_mut(package) {
        p.build_deps = helpers.iter().map(|h| (*h).to_owned()).collect();
    }
    g
}

fn subjects(g: &Graph) -> Vec<String> {
    let mut found: Vec<_> = deps::check(g, &[])
        .problems
        .into_iter()
        .map(|p| p.subject)
        .collect();
    found.sort();
    found
}

#[test]
fn an_audited_helper_is_excused_beneath_the_data_plane() {
    let g = reaching(
        "norn-shared-memory",
        "iceoryx2-pal-posix",
        &["cc", "bindgen"],
    );
    let f = deps::check(&g, &[]);
    assert!(f.problems.is_empty(), "{:?}", f.problems);
    assert!(f.examined > 0);
}

#[test]
fn the_same_helper_reached_from_an_engine_crate_fails() {
    let g = reaching("norn-core", "iceoryx2-pal-posix", &["cc"]);
    // Once for the helper, once for reaching iceoryx2 at all.
    assert_eq!(subjects(&g), ["iceoryx2-pal-posix", "iceoryx2-pal-posix"]);
}

#[test]
fn a_client_library_reaching_iceoryx2_fails_even_without_a_helper() {
    let g = reaching("norn-adapter", "iceoryx2", &[]);
    assert_eq!(subjects(&g), ["iceoryx2"]);
}

#[test]
fn an_unaudited_helper_of_an_audited_package_still_fails() {
    let g = reaching("norn-shared-memory", "iceoryx2-pal-posix", &["pkg-config"]);
    assert_eq!(subjects(&g), ["iceoryx2-pal-posix"]);
}

#[test]
fn an_audited_helper_excuses_no_other_package() {
    let g = reaching("norn-shared-memory", "iceoryx2-sys", &["cc"]);
    assert_eq!(subjects(&g), ["iceoryx2-sys"]);
}

#[test]
fn a_links_key_on_an_audited_package_is_never_excused() {
    let mut g = reaching("norn-server", "iceoryx2-pal-posix", &["cc"]);
    if let Some(p) = g.packages.get_mut("iceoryx2-pal-posix") {
        p.links = Some("iox2".into());
    }
    assert_eq!(subjects(&g), ["iceoryx2-pal-posix"]);
}
