//! The runtime-rs exception bites: ring only through the shared crates, only with
//! the runtime present, never for the engine, and OpenTelemetry only beneath the
//! daemon.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::graph;
use xtask::deps;
use xtask::metadata::{DepKind::Normal, Graph};

fn native(mut g: Graph) -> Graph {
    g.packages.get_mut("ring").unwrap().links = Some("ring_core_0_17_14_".into());
    g
}

fn subjects(g: &Graph) -> Vec<String> {
    let f = deps::check(g, &[]);
    f.problems.iter().map(|p| p.subject.clone()).collect()
}

#[test]
fn the_daemon_reaches_ring_through_the_runtime_and_a_client_through_rustls() {
    let g = native(graph(
        &["norn-server", "norn-cli"],
        &[
            ("norn-server", "tpp-grpc", Normal),
            ("tpp-grpc", "ring", Normal),
            ("norn-cli", "tonic", Normal),
            ("tonic", "tokio-rustls", Normal),
            ("tokio-rustls", "rustls", Normal),
            ("rustls", "ring", Normal),
        ],
    ));
    assert_eq!(subjects(&g), Vec::<String>::new());
}

#[test]
fn ring_without_the_runtime_in_the_graph_is_not_excused() {
    let g = native(graph(
        &["norn-cli"],
        &[("norn-cli", "rustls", Normal), ("rustls", "ring", Normal)],
    ));
    assert_eq!(subjects(&g), ["ring"]);
}

#[test]
fn a_client_naming_ring_itself_is_refused_even_with_the_runtime_present() {
    let g = native(graph(
        &["norn-server", "norn-cli"],
        &[
            ("norn-server", "tpp-grpc", Normal),
            ("tpp-grpc", "ring", Normal),
            ("norn-cli", "ring", Normal),
        ],
    ));
    assert_eq!(subjects(&g), ["ring"]);
}

#[test]
fn an_engine_crate_reaching_tls_or_the_runtime_fails_the_gate() {
    for reached in ["rustls", "tpp-grpc", "opentelemetry"] {
        let g = graph(
            &["norn-server", "norn-query"],
            &[
                ("norn-server", "tpp-agents", Normal),
                ("norn-query", reached, Normal),
            ],
        );
        assert!(subjects(&g).contains(&reached.to_owned()), "{reached}");
    }
}

#[test]
fn only_the_daemon_may_reach_opentelemetry() {
    let edges = [("norn-x", "opentelemetry", Normal)];
    assert_eq!(subjects(&graph(&["norn-x"], &edges)), ["opentelemetry"]);
    let edges = [("norn-server", "opentelemetry", Normal)];
    assert_eq!(
        subjects(&graph(&["norn-server"], &edges)),
        Vec::<String>::new()
    );
}

#[test]
fn this_repositorys_engine_crates_stay_free_of_all_of_it() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let g = xtask::metadata::load_for(root, deps::PLATFORMS).unwrap();
    let forbidden = ["ring", "rustls", "tpp-grpc", "tpp-agents", "opentelemetry"];
    for engine in xtask::runtime_rs::ENGINE {
        let reached = xtask::runtime_rs::closure_avoiding(&g, engine, &[]);
        assert!(reached.contains(*engine), "the graph must hold {engine}");
        for name in forbidden {
            assert!(!reached.contains(name), "{engine} reaches {name}");
        }
    }
}
