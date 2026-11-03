//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Retention hooks list whole segments and delete nothing.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, chunk, sealed, unsealed};
use norn_core::Timestamp;
use norn_storage::{StorageOptions, segments_before, segments_over_size};

fn secs(s: i64) -> Timestamp {
    Timestamp::from_secs(s).unwrap()
}

#[test]
fn only_segments_entirely_before_the_cutoff_are_listed_and_none_are_deleted() {
    let dir = Scratch::new();
    let a = sealed(dir.path(), 0, &[chunk(1, 0, 10)]); // 0..=9
    let b = sealed(dir.path(), 1, &[chunk(1, 10, 10)]); // 10..=19
    unsealed(dir.path(), 2, &[chunk(1, 0, 1)]); // old, but may be being written
    let o = StorageOptions::default();
    let listed = |cut| segments_before(dir.path(), secs(cut), &o).unwrap();
    assert!(
        listed(9).is_empty(),
        "a sample at the cutoff keeps the segment"
    );
    assert_eq!(listed(10), vec![a.clone()]);
    assert_eq!(
        listed(19),
        vec![a.clone()],
        "one later sample keeps it whole"
    );
    assert_eq!(listed(20), vec![a.clone(), b.clone()]);
    assert!(a.path.exists() && b.path.exists());
}

#[test]
fn a_size_limit_lists_the_oldest_segments_needed_and_deletes_nothing() {
    let dir = Scratch::new();
    let a = sealed(dir.path(), 0, &[chunk(1, 0, 10)]);
    let b = sealed(dir.path(), 1, &[chunk(1, 10, 10)]);
    let o = StorageOptions::default();
    let total = a.bytes + b.bytes;
    assert!(
        segments_over_size(dir.path(), total, &o)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        segments_over_size(dir.path(), total - 1, &o).unwrap(),
        vec![a.clone()]
    );
    assert_eq!(
        segments_over_size(dir.path(), 0, &o).unwrap(),
        vec![a.clone(), b.clone()]
    );
    assert!(a.path.exists() && b.path.exists());
}
