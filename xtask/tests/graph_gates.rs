//! `deps` and `arch` bite on hand-built graphs and pass on this repository.

mod common;

use common::graph;
use xtask::metadata::DepKind::{Build, Dev, Normal};
use xtask::{arch, deps, metadata, scan};

#[test]
fn a_links_key_anywhere_in_the_normal_graph_fails_deps() {
    let mut g = graph(
        &["norn-core"],
        &[("norn-core", "a", Normal), ("a", "sys", Normal)],
    );
    g.packages.get_mut("sys").unwrap().links = Some("z".into());
    let f = deps::check(&g, &[]);
    assert_eq!(f.problems.len(), 1);
    assert_eq!(f.problems[0].subject, "sys");
    assert!(f.verdict().is_err());
}

#[test]
fn a_cc_build_dependency_fails_deps() {
    let mut g = graph(&["norn-core"], &[("norn-core", "a", Normal)]);
    g.packages.get_mut("a").unwrap().build_deps = vec!["cc".into()];
    assert_eq!(deps::check(&g, &[]).problems[0].subject, "a");
}

#[test]
fn c_reached_only_through_dev_or_build_edges_does_not_ship() {
    let mut g = graph(
        &["norn-core"],
        &[("norn-core", "t", Dev), ("norn-core", "b", Build)],
    );
    g.packages.get_mut("t").unwrap().links = Some("z".into());
    g.packages.get_mut("b").unwrap().links = Some("z".into());
    let f = deps::check(&g, &[]);
    assert!(f.problems.is_empty());
    assert!(f.verdict().is_ok());
}

#[test]
fn an_allow_listed_member_may_reach_c() {
    let mut g = graph(&["norn-x"], &[("norn-x", "sys", Normal)]);
    g.packages.get_mut("sys").unwrap().links = Some("z".into());
    let f = deps::check(&g, &[("norn-x", "audited")]);
    assert!(f.problems.is_empty());
}

#[test]
fn a_graph_gate_over_no_members_fails() {
    let g = graph(&[], &[]);
    assert!(deps::check(&g, &[]).verdict().is_err());
    assert!(arch::check(&g, arch::LAYERS).verdict().is_err());
}

#[test]
fn an_outward_dependency_fails_arch() {
    let g = graph(
        &["norn-core", "norn-codec"],
        &[("norn-core", "norn-codec", Normal)],
    );
    let f = arch::check(&g, arch::LAYERS);
    assert_eq!(f.problems.len(), 1);
    assert_eq!(f.problems[0].subject, "norn-core");
}

#[test]
fn an_inward_dependency_and_an_outward_dev_dependency_pass_arch() {
    let edges = [
        ("norn-codec", "norn-core", Normal),
        ("norn-core", "norn-codec", Dev),
    ];
    let g = graph(&["norn-core", "norn-codec"], &edges);
    assert!(arch::check(&g, arch::LAYERS).verdict().is_ok());
}

#[test]
fn a_crate_missing_from_the_layer_table_fails_arch() {
    let g = graph(&["norn-core", "norn-unplaced"], &[]);
    let f = arch::check(&g, arch::LAYERS);
    assert_eq!(f.problems.len(), 1);
    assert_eq!(f.problems[0].subject, "norn-unplaced");
}

#[test]
fn the_layer_table_names_only_crates_it_also_places() {
    for (name, allowed) in arch::LAYERS {
        for dep in *allowed {
            assert!(
                arch::LAYERS.iter().any(|(n, _)| n == dep),
                "{name} allows unplaced {dep}"
            );
        }
    }
}

#[test]
fn cargo_metadata_json_parses_into_the_graph_model() {
    let json = serde_json::json!({
        "packages": [
            {"id": "a 1", "name": "a", "links": null, "dependencies": [{"name": "cc", "kind": "build"}],
             "targets": [{"kind": ["lib"], "src_path": "/r/a/src/lib.rs"}]},
            {"id": "b 1", "name": "b", "links": "z", "dependencies": [], "targets": []}
        ],
        "workspace_members": ["a 1"],
        "resolve": {"nodes": [
            {"id": "a 1", "deps": [{"pkg": "b 1", "dep_kinds": [{"kind": null}, {"kind": "dev"}]}]}
        ]}
    });
    let g = metadata::parse(&json).unwrap();
    assert_eq!(g.members, ["a 1"]);
    assert_eq!(g.packages["a 1"].build_deps, ["cc"]);
    assert_eq!(g.packages["b 1"].links.as_deref(), Some("z"));
    assert_eq!(g.edges["a 1"][0].kinds, [Normal, Dev]);
    assert!(
        metadata::parse(&serde_json::json!({"packages": []})).is_err(),
        "no resolve graph"
    );
}

#[test]
fn this_repository_passes_the_graph_gates_and_examines_something() {
    let root = scan::repo_root().unwrap();
    for gate in ["arch", "deps", "docs"] {
        let f = xtask::run(gate, &root).unwrap();
        assert!(f.examined > 0, "{gate} examined nothing");
        assert!(f.verdict().is_ok(), "{:?}", f.verdict());
    }
}
