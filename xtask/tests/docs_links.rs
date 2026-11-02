//! `docs` over Markdown: links, images and anchors.

use std::collections::BTreeSet;
use std::path::PathBuf;

use xtask::docs::{headings, links, markdown};

fn tracked(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn doc(path: &str, text: &str) -> (PathBuf, String) {
    (PathBuf::from(path), text.to_owned())
}

#[test]
fn a_link_to_a_missing_file_fails_and_names_its_line() {
    let docs = [doc("docs/a.md", "ok [b](b.md)\n\nbad [c](c.md)\n")];
    let f = links::check(&docs, &tracked(&["docs/a.md", "docs/b.md"]));
    assert_eq!(f.examined, 2);
    assert_eq!(f.problems.len(), 1);
    assert_eq!(f.problems[0].subject, "docs/a.md:3");
}

#[test]
fn an_image_a_reference_and_html_are_links_too() {
    let text = "![i](i.png)\n\n[r]: r.md\n\n<img src=\"h.png\">\n";
    let f = links::check(&[doc("a.md", text)], &tracked(&["a.md"]));
    assert_eq!(f.problems.len(), 3, "{f:?}");
}

#[test]
fn links_climb_out_of_a_directory_and_may_name_a_directory() {
    let text = "[s](../../crates/x/src/lib.rs) [d](../../crates)\n";
    let f = links::check(
        &[doc("docs/guides/a.md", text)],
        &tracked(&["docs/guides/a.md", "crates/x/src/lib.rs"]),
    );
    assert_eq!(f.examined, 2);
    assert!(f.verdict().is_ok(), "{:?}", f.verdict());
}

#[test]
fn a_link_above_the_root_fails() {
    let f = links::check(&[doc("a.md", "[x](../a.md)\n")], &tracked(&["a.md"]));
    assert_eq!(f.problems.len(), 1);
}

#[test]
fn a_prefix_of_a_file_name_is_not_a_directory() {
    let f = links::check(
        &[doc("a.md", "[x](crates)\n")],
        &tracked(&["a.md", "crates-old"]),
    );
    assert_eq!(f.problems.len(), 1);
}

#[test]
fn a_link_differing_only_in_case_fails_as_it_would_on_github() {
    let f = links::check(&[doc("a.md", "[x](B.md)\n")], &tracked(&["a.md", "b.md"]));
    assert_eq!(f.problems.len(), 1);
}

#[test]
fn a_missing_anchor_fails_in_this_file_and_in_another() {
    let docs = [
        doc(
            "a.md",
            "# One\n\n[ok](#one) [bad](#two)\n[far](b.md#nope)\n",
        ),
        doc("b.md", "## Real\n\n[back](a.md#one) [ok](a.md#real)\n"),
    ];
    let f = links::check(&docs, &tracked(&["a.md", "b.md"]));
    let subjects: Vec<_> = f.problems.iter().map(|p| p.subject.as_str()).collect();
    assert_eq!(subjects, ["a.md:3", "a.md:4", "b.md:3"], "{f:?}");
}

#[test]
fn an_anchor_into_source_is_not_checked_as_a_heading() {
    let f = links::check(
        &[doc("a.md", "[x](b.rs#L3)\n")],
        &tracked(&["a.md", "b.rs"]),
    );
    assert!(f.verdict().is_ok());
}

#[test]
fn links_in_code_are_examples_and_external_links_are_not_checked() {
    let text = "```md\n[x](gone.md)\n```\n`[y](gone.md)`\n[e](https://a.b/c) [m](mailto:a@b.c)\n";
    assert!(markdown::links(text).len() == 2);
    let f = links::check(&[doc("a.md", text)], &tracked(&["a.md"]));
    assert_eq!(f.examined, 0, "nothing relative was seen");
    assert!(f.verdict().is_err());
}

#[test]
fn code_in_a_label_keeps_the_link() {
    let links = markdown::links("[`protobuf/`](protobuf)\n");
    assert_eq!(links[0].target, "protobuf");
}

#[test]
fn slugs_match_what_github_generates() {
    assert_eq!(
        headings::slug("Process model (exists: stage 4)"),
        "process-model-exists-stage-4"
    );
    assert_eq!(
        headings::slug("Ingestion: `TelemetryService.AppendSamples`"),
        "ingestion-telemetryserviceappendsamples"
    );
    assert_eq!(headings::slug("A & B"), "a--b");
    assert_eq!(
        headings::slug("max_chunk_age [link](x.md) too"),
        "max_chunk_age-link-too"
    );
}

#[test]
fn a_repeated_heading_gets_a_numbered_anchor() {
    let anchors = headings::anchors("# A\n## A\n```\n# Not\n```\n");
    assert_eq!(anchors, tracked(&["a", "a-1"]));
}
