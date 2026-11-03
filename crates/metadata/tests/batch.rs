//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Batch creates: all or nothing, safe to repeat, bounded.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Dir, open, seed, t};
use norn_core::{Limits, MetadataLimits, ValueKind};
use norn_metadata::{
    EntitySpec, MAX_BATCH_CREATE, MetadataError, Registry, SignalSpec, StreamSpec, Write,
};

fn signals(entity: &str, n: usize) -> Vec<(String, String, SignalSpec)> {
    (0..n)
        .map(|i| {
            let spec = SignalSpec::new(ValueKind::F64, "plant".into());
            (entity.to_owned(), format!("s-{i}"), spec)
        })
        .collect()
}

fn log_len(dir: &Dir) -> u64 {
    std::fs::metadata(dir.0.join("metadata.log")).unwrap().len()
}

#[test]
fn a_batch_creates_every_signal_with_its_own_id_and_survives_a_restart() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let made = r
        .create_signals(signals("ahu-01", 500), t(2), Write::Commit)
        .unwrap();
    assert_eq!(made.len(), 500);
    assert_eq!(made[7].descriptor.name.as_str(), "s-7");
    let mut ids: Vec<_> = made.iter().map(|s| s.descriptor.id).collect();
    ids.push(r.signal("ahu-01", "temp").unwrap().descriptor.id);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 501, "every signal has its own compact ID");
    drop(r);
    let r = open(&dir.0);
    assert_eq!(
        r.list_signals(Some("ahu-01"), None, 1000, |_| true).len(),
        501
    );
    let back = r.signal("ahu-01", "s-499").unwrap();
    assert_eq!(back.descriptor.id, made[499].descriptor.id);
    assert!(r.view().route("entities/ahu-01/signals/s-499").is_some());
}

#[test]
fn repeating_a_batch_returns_what_exists_and_writes_nothing() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let first = r
        .create_signals(signals("ahu-01", 50), t(2), Write::Commit)
        .unwrap();
    let before = log_len(&dir);
    let again = r
        .create_signals(signals("ahu-01", 50), t(3), Write::Commit)
        .unwrap();
    assert_eq!(log_len(&dir), before);
    for (a, b) in first.iter().zip(&again) {
        assert_eq!(a.descriptor.id, b.descriptor.id);
        assert_eq!(b.create_time, t(2), "the stored record, untouched");
    }
    let mixed = r
        .create_entities(
            vec![
                ("ahu-01".into(), EntitySpec::default()),
                ("ahu-02".into(), EntitySpec::default()),
            ],
            t(4),
            Write::Commit,
        )
        .unwrap();
    assert_eq!(mixed[0].create_time, t(1));
    assert_eq!(mixed[1].create_time, t(4));
}

#[test]
fn a_signal_that_disagrees_on_its_value_kind_or_stream_fails_the_whole_batch() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    r.create_stream("other", StreamSpec::default(), t(1), Write::Commit)
        .unwrap();
    for spec in [
        SignalSpec::new(ValueKind::Bool, "plant".into()),
        SignalSpec::new(ValueKind::F64, "other".into()),
    ] {
        let mut batch = signals("ahu-01", 3);
        batch.push(("ahu-01".into(), "temp".into(), spec));
        let e = r.create_signals(batch, t(2), Write::Commit);
        assert!(
            matches!(e, Err(MetadataError::AlreadyExists { .. })),
            "{e:?}"
        );
        assert!(r.signal("ahu-01", "s-0").is_err(), "nothing was created");
    }
}

#[test]
fn a_missing_parent_a_repeated_id_or_a_bad_id_creates_nothing() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let mut missing = signals("ahu-01", 3);
    missing.extend(signals("nope", 1));
    let e = r.create_signals(missing, t(2), Write::Commit);
    assert!(matches!(e, Err(MetadataError::NotFound { .. })), "{e:?}");
    let mut twice = signals("ahu-01", 3);
    twice.extend(signals("ahu-01", 1));
    let e = r.create_signals(twice, t(2), Write::Commit);
    assert!(matches!(e, Err(MetadataError::Invalid { .. })), "{e:?}");
    let bad = vec![
        ("ok".to_owned(), EntitySpec::default()),
        ("Bad".to_owned(), EntitySpec::default()),
    ];
    let e = r.create_entities(bad, t(2), Write::Commit);
    assert!(matches!(e, Err(MetadataError::Invalid { .. })), "{e:?}");
    assert!(r.signal("ahu-01", "s-0").is_err());
    assert!(r.entity("ok").is_err());
}

#[test]
fn a_batch_past_a_limit_is_refused_whole() {
    let dir = Dir::new();
    let mut limits = Limits::default();
    limits.max_signals = 10;
    let (r, _) = Registry::open(&dir.0, limits, MetadataLimits::default()).unwrap();
    seed(&r);
    let e = r.create_signals(signals("ahu-01", 10), t(2), Write::Commit);
    assert!(matches!(
        e,
        Err(MetadataError::LimitExceeded {
            limit: "max_signals",
            ..
        })
    ));
    assert_eq!(r.list_signals(None, None, 100, |_| true).len(), 1);
    let e = r.create_signals(signals("ahu-01", MAX_BATCH_CREATE + 1), t(2), Write::Commit);
    assert!(matches!(
        e,
        Err(MetadataError::LimitExceeded {
            limit: "max_batch_create",
            ..
        })
    ));
}

#[test]
fn validate_only_answers_with_the_records_and_stores_nothing() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let before = log_len(&dir);
    let made = r
        .create_signals(signals("ahu-01", 5), t(2), Write::ValidateOnly)
        .unwrap();
    assert_eq!(made.len(), 5);
    assert!(r.signal("ahu-01", "s-0").is_err());
    assert_eq!(log_len(&dir), before);
}
