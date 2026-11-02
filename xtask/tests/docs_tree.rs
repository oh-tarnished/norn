//! `docs` over the tree: the contents table, the one README, the whole gate.

use std::collections::BTreeSet;

use xtask::docs::{self, contents, emoji, links, readmes};
use xtask::scan::{self, Findings};

fn tracked(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn details(f: &Findings) -> Vec<String> {
    f.problems.iter().map(|p| p.detail.clone()).collect()
}

#[test]
fn a_docs_file_missing_from_the_contents_fails() {
    let readme = "# N\n\n## Contents\n\n- [a](docs/a.md)\n\n## Next\n\n[b](docs/b.md)\n";
    let f = contents::check(
        Some(readme),
        &tracked(&["docs/a.md", "docs/b.md", "README.md"]),
    );
    assert_eq!(f.examined, 2);
    assert_eq!(details(&f), ["contents omit `docs/b.md`"]);
}

#[test]
fn a_contents_entry_outside_docs_tree_fails_in_reverse() {
    let readme = "## Contents\n[a](docs/a.md) [g](docs/gone.md#x) [c](CONTRIBUTING.md)\n";
    let f = contents::check(Some(readme), &tracked(&["docs/a.md"]));
    assert_eq!(details(&f), ["contents list `docs/gone.md`, not in docs/"]);
}

#[test]
fn a_readme_with_no_contents_section_fails_and_so_does_no_readme() {
    let paths = tracked(&["docs/a.md"]);
    assert_eq!(contents::check(Some("# N\n"), &paths).problems.len(), 1);
    assert!(contents::check(None, &paths).verdict().is_err());
}

#[test]
fn a_contents_check_over_nothing_fails() {
    let f = contents::check(Some("## Contents\n"), &BTreeSet::new());
    assert_eq!(f.examined, 0);
    assert!(f.verdict().is_err());
}

#[test]
fn a_readme_below_the_root_fails_however_it_is_spelled() {
    let paths = tracked(&[
        "README.md",
        "crates/a/README.md",
        "docs/readme.txt",
        "x/Readme",
    ]);
    let f = readmes::check(&paths);
    let subjects: Vec<_> = f.problems.iter().map(|p| p.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["crates/a/README.md", "docs/readme.txt", "x/Readme"]
    );
}

#[test]
fn a_root_without_a_readme_fails_and_a_file_merely_named_like_one_does_not() {
    assert_eq!(readmes::check(&tracked(&["a.md"])).problems.len(), 1);
    assert!(
        readmes::check(&tracked(&["README.md", "a/readmes.md"]))
            .verdict()
            .is_ok()
    );
}

#[test]
fn the_new_checks_over_nothing_fail() {
    assert!(links::check(&[], &BTreeSet::new()).verdict().is_err());
    assert!(readmes::check(&BTreeSet::new()).verdict().is_err());
}

#[test]
fn one_empty_check_fails_the_gate_however_much_the_others_examined() {
    let mut busy = Findings::new("busy");
    busy.examined = 10;
    let combined = docs::combine(vec![busy, Findings::new("idle")]);
    assert_eq!(combined.examined, 10);
    assert_eq!(combined.problems[0].subject, "idle");
    assert!(combined.verdict().is_err());
}

#[test]
fn this_repository_passes_docs_and_examines_something() {
    let root = scan::repo_root().unwrap();
    let f = xtask::run("docs", &root).unwrap();
    assert!(f.examined > 100, "only {} examined", f.examined);
    assert!(f.verdict().is_ok(), "{:?}", f.verdict());
}

#[test]
fn a_check_mark_in_the_readme_fails_with_its_line_and_code_point() {
    let readme = "# N\n\n| Stage | State |\n| 1 | \u{2705} |\n| 2 | Done |\n";
    let f = emoji::check(Some(readme));
    assert_eq!(f.examined, 5);
    let subjects: Vec<_> = f.problems.iter().map(|p| p.subject.as_str()).collect();
    assert_eq!(subjects, ["README.md:4"]);
    assert!(details(&f)[0].contains("U+2705"));
    assert!(f.verdict().is_err());
}

#[test]
fn a_construction_sign_and_a_pictograph_with_its_selector_both_fail() {
    let f = emoji::check(Some("\u{1F6A7} planned\n\u{2714}\u{FE0F} done\n"));
    assert_eq!(f.problems.len(), 3);
}

#[test]
fn typographic_symbols_are_not_pictographs_and_pass() {
    let readme = "Copyright \u{a9} 2026. 16,000 \u{d7} 1 Hz, 49.5 \u{b5}s, a \u{2192} b \u{2014} c.\n";
    assert!(emoji::check(Some(readme)).verdict().is_ok());
}

#[test]
fn an_empty_or_missing_readme_fails_the_emoji_check() {
    assert!(emoji::check(Some("")).verdict().is_err());
    assert!(emoji::check(None).verdict().is_err());
}
