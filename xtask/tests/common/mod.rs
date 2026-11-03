//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Hand-built graphs for the graph gates' tests.

use xtask::metadata::{DepKind, Edge, Graph, Package};

/// A graph with `members` as workspace members and every other name in
/// `edges` as a registry package. Ids are the names.
pub fn graph(members: &[&str], edges: &[(&str, &str, DepKind)]) -> Graph {
    let mut g = Graph::default();
    let mut add = |name: &str| {
        g.packages
            .entry(name.to_owned())
            .or_insert_with(|| Package {
                name: name.to_owned(),
                ..Package::default()
            });
    };
    for name in members {
        add(name);
    }
    for (from, to, _) in edges {
        add(from);
        add(to);
    }
    g.members = members.iter().map(|m| (*m).to_owned()).collect();
    for (from, to, kind) in edges {
        g.edges.entry((*from).to_owned()).or_default().push(Edge {
            to: (*to).to_owned(),
            kinds: vec![*kind],
        });
    }
    g
}
