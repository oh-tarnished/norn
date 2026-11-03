//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `size`, `purity` and `docs` bite on synthetic input and pass on this
//! repository.

use std::path::PathBuf;

use xtask::{docs, purity, scan, size};

fn file(path: &str, lines: usize) -> (PathBuf, String) {
    (PathBuf::from(path), "x\n".repeat(lines))
}

#[test]
fn a_source_file_of_two_hundred_lines_fails_size() {
    let f = size::check(&[
        file("crates/a/src/big.rs", 200),
        file("crates/a/src/ok.rs", 199),
    ]);
    assert_eq!(f.examined, 2);
    assert_eq!(f.problems.len(), 1);
    assert_eq!(f.problems[0].subject, "crates/a/src/big.rs");
}

#[test]
fn proto_python_and_shell_files_are_measured_too() {
    for path in ["proto/norn/a.proto", "tools/a.py", "scripts/a.sh"] {
        assert_eq!(size::check(&[file(path, 250)]).problems.len(), 1, "{path}");
    }
}

#[test]
fn generated_code_and_markdown_are_exempt_from_size() {
    let f = size::check(&[
        file("crates/protocol/src/generated/norn.rs", 5000),
        file("docs/STORAGE.md", 5000),
        file("crates/a/src/lib.rs", 1),
    ]);
    assert_eq!(f.examined, 1);
    assert!(f.verdict().is_ok());
}

#[test]
fn a_tree_gate_over_no_files_fails() {
    assert!(size::check(&[]).verdict().is_err());
    assert!(purity::check(&[]).verdict().is_err());
    assert!(docs::check(&[]).verdict().is_err());
}

#[test]
fn reading_a_clock_or_a_file_in_the_core_fails_purity() {
    let code = "use std::fs::File;\nlet t = std::time::Instant::now();\n".to_owned();
    let f = purity::check(&[(PathBuf::from("crates/core/src/a.rs"), code)]);
    assert_eq!(f.problems.len(), 2);
    assert_eq!(f.problems[0].subject, "crates/core/src/a.rs:1");
}

#[test]
fn a_comment_may_name_what_the_core_must_not_do() {
    let code = "//! Never touches std::fs.\n// nor Instant\nfn a() {}\n".to_owned();
    assert!(
        purity::check(&[(PathBuf::from("a.rs"), code)])
            .verdict()
            .is_ok()
    );
}

#[test]
fn a_crate_root_without_a_leading_contract_fails_docs() {
    let roots = [
        (
            "good".to_owned(),
            PathBuf::from("g/lib.rs"),
            "\n//! Contract.\nmod a;\n".to_owned(),
        ),
        (
            "bad".to_owned(),
            PathBuf::from("b/lib.rs"),
            "mod a;\n//! Too late.\n".to_owned(),
        ),
        (
            "doc".to_owned(),
            PathBuf::from("d/lib.rs"),
            "/// Item doc.\nfn a() {}\n".to_owned(),
        ),
    ];
    let f = docs::check(&roots);
    let failed: Vec<_> = f.problems.iter().map(|p| p.subject.as_str()).collect();
    assert_eq!(failed, ["bad", "doc"]);
}

#[test]
fn the_walker_skips_target_and_git_but_not_github() {
    let root = scan::repo_root().unwrap();
    let files = scan::files(&root);
    let shown: Vec<_> = files.iter().map(|p| scan::display(p)).collect();
    assert!(shown.iter().any(|p| p.starts_with(".github/")));
    assert!(
        !shown
            .iter()
            .any(|p| p.starts_with("target/") || p.starts_with(".git/"))
    );
}

#[test]
fn this_repository_passes_the_tree_gates_and_examines_something() {
    let root = scan::repo_root().unwrap();
    for gate in ["size", "purity"] {
        let f = xtask::run(gate, &root).unwrap();
        assert!(f.examined > 0, "{gate} examined nothing");
        assert!(f.verdict().is_ok(), "{:?}", f.verdict());
    }
}

#[test]
fn an_unknown_gate_is_an_error_not_a_pass() {
    assert!(xtask::run("sizes", &scan::repo_root().unwrap()).is_err());
}
