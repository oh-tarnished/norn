//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Batches on disk: a crash keeps all of one or none, the version 1 log
//! upgrades, and group commit holds under concurrent batches and creates.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::sync::Arc;

use common::{Dir, open, seed, t};
use norn_core::{Limits, MetadataLimits, ValueKind};
use norn_metadata::{EntitySpec, MetadataError, Registry, SignalSpec, Write};

fn batch(entity: &str, n: usize) -> Vec<(String, String, SignalSpec)> {
    let spec = |_| SignalSpec::new(ValueKind::F64, "plant".into());
    (0..n)
        .map(|i| (entity.to_owned(), format!("b-{i}"), spec(i)))
        .collect()
}

fn reopen(dir: &Dir) -> Result<Registry, MetadataError> {
    Registry::open(&dir.0, Limits::default(), MetadataLimits::default()).map(|(r, _)| r)
}

/// CRC-32C (Castagnoli), bitwise: enough to forge a header in a test.
fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0x82F6_3B78
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn a_crash_anywhere_inside_a_batch_loses_all_of_it_and_nothing_before() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    drop(r);
    let log = dir.0.join("metadata.log");
    let before = std::fs::read(&log).unwrap().len();
    let r = open(&dir.0);
    r.create_signals(batch("ahu-01", 20), t(2), Write::Commit)
        .unwrap();
    drop(r);
    let whole = std::fs::read(&log).unwrap();
    // Cut inside the marker, inside a middle record, and inside the last.
    for keep in [before + 3, (before + whole.len()) / 2, whole.len() - 1] {
        std::fs::write(&log, &whole[..keep]).unwrap();
        let (r, report) =
            Registry::open(&dir.0, Limits::default(), MetadataLimits::default()).unwrap();
        assert_eq!(report.torn_bytes, (keep - before) as u64, "cut at {keep}");
        assert!(r.signal("ahu-01", "temp").is_ok());
        assert!(r.list_signals(Some("ahu-01"), None, 100, |_| true).len() == 1);
    }
    std::fs::write(&log, &whole).unwrap();
    let r = reopen(&dir).unwrap();
    assert_eq!(
        r.list_signals(Some("ahu-01"), None, 100, |_| true).len(),
        21
    );
}

#[test]
fn a_version_1_log_opens_is_upgraded_and_then_refused_by_a_version_1_reader() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    drop(r);
    let log = dir.0.join("metadata.log");
    let mut bytes = std::fs::read(&log).unwrap();
    assert_eq!(&bytes[8..12], &2u32.to_le_bytes(), "new logs are version 2");
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    let crc = crc32c(&bytes[..12]);
    bytes[12..16].copy_from_slice(&crc.to_le_bytes());
    std::fs::write(&log, &bytes).unwrap();
    let r = reopen(&dir).unwrap();
    assert!(r.signal("ahu-01", "temp").is_ok());
    drop(r);
    let upgraded = std::fs::read(&log).unwrap();
    assert_eq!(&upgraded[8..12], &2u32.to_le_bytes());
    assert_eq!(&upgraded[16..], &bytes[16..], "only the header changed");
    let mut newer = upgraded;
    newer[8..12].copy_from_slice(&3u32.to_le_bytes());
    let crc = crc32c(&newer[..12]);
    newer[12..16].copy_from_slice(&crc.to_le_bytes());
    std::fs::write(&log, &newer).unwrap();
    let e = reopen(&dir);
    assert!(matches!(
        e,
        Err(MetadataError::UnsupportedVersion { found: 3 })
    ));
}

#[test]
fn concurrent_batches_and_single_creates_all_land_and_replay_the_same() {
    let dir = Dir::new();
    let r = Arc::new(open(&dir.0));
    seed(&r);
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let r = Arc::clone(&r);
            std::thread::spawn(move || {
                let entity = format!("e-{n}");
                r.create_entities(
                    vec![(entity.clone(), EntitySpec::default())],
                    t(1),
                    Write::Commit,
                )
                .unwrap();
                for chunk in 0..5 {
                    let mut b = batch(&entity, 40);
                    b.iter_mut().for_each(|s| s.1 = format!("{}-{chunk}", s.1));
                    r.create_signals(b, t(2), Write::Commit).unwrap();
                    let spec = SignalSpec::new(ValueKind::F64, "plant".into());
                    let id = format!("one-{chunk}");
                    r.create_signal(&entity, &id, spec, t(2), Write::Commit)
                        .unwrap();
                }
            })
        })
        .collect();
    threads.into_iter().for_each(|h| h.join().unwrap());
    let all = r.list_signals(None, None, 5_000, |_| true);
    assert_eq!(all.len(), 1 + 8 * 5 * 41);
    let ids: Vec<_> = all.iter().map(|(_, s)| s.descriptor.id).collect();
    drop(r);
    let back = reopen(&dir)
        .unwrap()
        .list_signals(None, None, 5_000, |_| true);
    let back: Vec<_> = back.iter().map(|(_, s)| s.descriptor.id).collect();
    assert_eq!(back, ids);
}
