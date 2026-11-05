//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry on disk: the lock, torn tails, damage, rewriting.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::sync::Arc;

use common::{Dir, open, seed, t};
use norn_core::{Limits, Metadata, MetadataLimits};
use norn_metadata::{EntityPatch, EntitySpec, MetadataError, Registry, Write};

#[test]
fn a_second_registry_on_a_directory_is_locked_out_until_the_first_is_gone() {
    let dir = Dir::new();
    let first = open(&dir.0);
    let e = Registry::open(&dir.0, Limits::default(), MetadataLimits::default());
    assert!(matches!(e, Err(MetadataError::Locked { .. })));
    drop(first);
    drop(open(&dir.0));
}

#[test]
fn a_torn_tail_is_kept_aside_and_cut_and_earlier_records_survive() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    drop(r);
    let log = dir.0.join("metadata.log");
    let whole = std::fs::read(&log).unwrap();
    for cut in [1usize, 7, 30] {
        std::fs::write(&log, &whole[..whole.len() - cut]).unwrap();
        let (r, report) =
            Registry::open(&dir.0, Limits::default(), MetadataLimits::default()).unwrap();
        assert!(report.torn_bytes > 0);
        let side = report.torn_to.clone().unwrap();
        assert!(side.exists());
        assert!(r.entity("dev-01").is_ok());
        assert!(r.signal("dev-01", "temp").is_err());
    }
}

#[test]
fn damage_before_the_tail_refuses_to_open_rather_than_lose_later_records() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    drop(r);
    let log = dir.0.join("metadata.log");
    let mut bytes = std::fs::read(&log).unwrap();
    bytes[30] ^= 0xff;
    std::fs::write(&log, &bytes).unwrap();
    let e = Registry::open(&dir.0, Limits::default(), MetadataLimits::default());
    assert!(matches!(e, Err(MetadataError::Corrupt { .. })));
}

#[test]
fn a_log_full_of_dead_records_is_rewritten_and_replays_the_same() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    for i in 0..1_500 {
        let patch = EntityPatch::new(Some(format!("name {i}")), Some(Metadata::default()));
        r.update_entity("dev-01", patch, None, t(i), Write::Commit)
            .unwrap();
    }
    let len = std::fs::metadata(dir.0.join("metadata.log")).unwrap().len();
    assert!(len < 120_000, "log was not rewritten: {len} bytes");
    let last = r.entity("dev-01").unwrap();
    drop(r);
    let back = open(&dir.0).entity("dev-01").unwrap();
    assert_eq!(back.display_name, last.display_name);
    assert_eq!(back.revision, last.revision);
}

#[test]
fn concurrent_creates_all_land_and_share_fsyncs() {
    let dir = Dir::new();
    let r = Arc::new(open(&dir.0));
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let r = Arc::clone(&r);
            std::thread::spawn(move || {
                for i in 0..20 {
                    let id = format!("e-{n}-{i}");
                    r.create_entity(&id, EntitySpec::default(), t(1), Write::Commit)
                        .unwrap();
                }
            })
        })
        .collect();
    threads.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(r.list_entities(None, 1000, |_| true).len(), 160);
    drop(r);
    assert_eq!(open(&dir.0).list_entities(None, 1000, |_| true).len(), 160);
}
