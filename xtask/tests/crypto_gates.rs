//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Encryption's crates stay beneath norn-storage, as xtask/src/crypto.rs says.

mod common;

use common::graph;
use xtask::deps;
use xtask::metadata::DepKind::{Dev, Normal};

fn subjects(members: &[&str], edges: &[(&str, &str, xtask::metadata::DepKind)]) -> Vec<String> {
    deps::check(&graph(members, edges), &[])
        .problems
        .into_iter()
        .map(|p| p.subject)
        .collect()
}

#[test]
fn norn_storage_may_name_a_cipher() {
    let edges = [("norn-storage", "chacha20poly1305", Normal)];
    assert!(subjects(&["norn-storage"], &edges).is_empty());
}

#[test]
fn another_member_naming_a_cipher_fails() {
    let edges = [("norn-server", "chacha20poly1305", Normal)];
    assert_eq!(subjects(&["norn-server"], &edges), ["chacha20poly1305"]);
    let edges = [("norn-core", "zeroize", Normal)];
    assert_eq!(subjects(&["norn-core"], &edges), ["zeroize"]);
}

#[test]
fn a_test_may_name_one_since_it_never_ships() {
    let edges = [("norn-server", "hkdf", Dev)];
    assert!(subjects(&["norn-server"], &edges).is_empty());
}

#[test]
fn an_impure_cipher_beneath_norn_storage_fails() {
    let edges = [("norn-storage", "ring", Normal)];
    assert!(subjects(&["norn-storage"], &edges).contains(&"ring".to_owned()));
}
