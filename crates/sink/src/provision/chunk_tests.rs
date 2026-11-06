//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::{Resource, cut};
use norn_protocol::signal::Signal;
use std::collections::HashMap;

/// Three quarters of 256 KiB, the cut `Options::default` makes.
const CUT: usize = 192 << 10;

fn signal(n: usize, text: usize) -> Resource {
    let annotations: HashMap<String, String> =
        [("source.description".to_owned(), "x".repeat(text))].into();
    Resource::Signal {
        parent: "entities/e1".to_owned(),
        id: format!("s{n}"),
        signal: Signal {
            name: format!("entities/e1/signals/s{n}"),
            annotations,
            ..Signal::default()
        },
    }
}

#[test]
fn a_batch_is_cut_to_the_message_limit_not_only_its_count() {
    let small: Vec<Resource> = (0..2500).map(|n| signal(n, 10)).collect();
    let sizes: Vec<usize> = cut(small, CUT).iter().map(Vec::len).collect();
    assert_eq!(sizes, [1000, 1000, 500], "by count");
    let large: Vec<Resource> = (0..1000).map(|n| signal(n, 1000)).collect();
    let batches = cut(large, CUT);
    assert!(
        batches.len() >= 6,
        "about 1 KiB each, so under 192 per batch"
    );
    assert!(
        batches.iter().all(|b| b.len() <= 192),
        "{:?}",
        batches.iter().map(Vec::len).collect::<Vec<_>>()
    );
    assert_eq!(
        batches.iter().map(Vec::len).sum::<usize>(),
        1000,
        "none lost"
    );
}

#[test]
fn a_resource_larger_than_the_limit_alone_still_goes_alone() {
    let batches = cut(vec![signal(0, 10), signal(1, 4096), signal(2, 10)], 1024);
    let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
    assert_eq!(sizes, [1, 1, 1]);
}
