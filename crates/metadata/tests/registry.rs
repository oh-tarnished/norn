//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry: CRUD, limits and refused deletes.
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
    EntityPatch, EntitySpec, MetadataError, Registry, SignalPatch, SignalSpec, StreamSpec, Write,
};

#[test]
fn records_survive_a_restart_with_their_ids_and_revisions() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let patch = EntityPatch::new(Some("Air handler".into()), None);
    let updated = r
        .update_entity("ahu-01", patch, Some(1), t(2), Write::Commit)
        .unwrap();
    assert_eq!(updated.revision, 2);
    let signal = r.signal("ahu-01", "temp").unwrap();
    drop(r);
    let r = open(&dir.0);
    let entity = r.entity("ahu-01").unwrap();
    assert_eq!(entity.display_name, updated.display_name);
    assert_eq!(entity.revision, 2);
    assert_eq!(entity.descriptor.id, updated.descriptor.id);
    let back = r.signal("ahu-01", "temp").unwrap();
    assert_eq!(back.descriptor.id, signal.descriptor.id);
    assert_eq!(back.stream, signal.stream);
    let view = r.view();
    let route = view.route("entities/ahu-01/signals/temp").unwrap();
    assert_eq!(route.id, signal.descriptor.id);
    assert_eq!(route.kind, ValueKind::F64);
}

#[test]
fn an_id_is_never_reused_after_a_delete_and_a_restart() {
    let dir = Dir::new();
    let r = open(&dir.0);
    let a = r
        .create_entity("a", EntitySpec::default(), t(1), Write::Commit)
        .unwrap();
    r.delete_entity("a", None, Write::Commit).unwrap();
    drop(r);
    let r = open(&dir.0);
    let b = r
        .create_entity("b", EntitySpec::default(), t(1), Write::Commit)
        .unwrap();
    assert_ne!(a.descriptor.id, b.descriptor.id);
}

#[test]
fn creating_a_name_twice_and_a_bad_id_are_refused() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let again = r.create_entity("ahu-01", EntitySpec::default(), t(1), Write::Commit);
    assert!(matches!(again, Err(MetadataError::AlreadyExists { .. })));
    for bad in ["", "Upper", "1digit", "under_score", &"x".repeat(64)] {
        let e = r.create_entity(bad, EntitySpec::default(), t(1), Write::Commit);
        assert!(matches!(e, Err(MetadataError::Invalid { .. })), "{bad}");
    }
}

#[test]
fn validate_only_returns_the_result_and_stores_nothing() {
    let dir = Dir::new();
    let r = open(&dir.0);
    let made = r
        .create_entity("a", EntitySpec::default(), t(1), Write::ValidateOnly)
        .unwrap();
    assert_eq!(made.descriptor.name.as_str(), "a");
    assert!(matches!(r.entity("a"), Err(MetadataError::NotFound { .. })));
}

#[test]
fn a_stale_revision_is_refused_and_changes_nothing() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let patch = SignalPatch::new(None, Some("Cel".into()), None);
    let e = r.update_signal("ahu-01", "temp", patch, Some(9), t(2), Write::Commit);
    assert!(matches!(
        e,
        Err(MetadataError::RevisionMismatch { current: 1 })
    ));
    assert_eq!(r.signal("ahu-01", "temp").unwrap().unit, "");
}

#[test]
fn deletes_refuse_anything_holding_children_or_data() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    let holds = |e| matches!(e, Err(MetadataError::NotEmpty { .. }));
    assert!(holds(r.delete_entity("ahu-01", None, Write::Commit)));
    assert!(holds(r.delete_stream("plant", None, Write::Commit)));
    r.view()
        .route("entities/ahu-01/signals/temp")
        .unwrap()
        .mark_data();
    assert!(holds(r.delete_signal(
        "ahu-01",
        "temp",
        None,
        Write::Commit
    )));
    drop(r);
    let r = open(&dir.0);
    r.delete_signal("ahu-01", "temp", None, Write::Commit)
        .unwrap();
    r.delete_entity("ahu-01", None, Write::Commit).unwrap();
    r.delete_stream("plant", None, Write::Commit).unwrap();
}

#[test]
fn listing_pages_in_name_order_and_across_entities() {
    let dir = Dir::new();
    let r = open(&dir.0);
    seed(&r);
    r.create_entity("ahu-02", EntitySpec::default(), t(1), Write::Commit)
        .unwrap();
    for id in ["b", "a"] {
        let spec = SignalSpec::new(ValueKind::I32, "plant".into());
        r.create_signal("ahu-02", id, spec, t(1), Write::Commit)
            .unwrap();
    }
    let page = r.list_entities(None, 1, |_| true);
    assert_eq!(page[0].descriptor.name.as_str(), "ahu-01");
    let page = r.list_entities(Some("ahu-01"), 10, |_| true);
    assert_eq!(page.len(), 1);
    let all = r.list_signals(None, None, 10, |_| true);
    let names: Vec<_> = all
        .iter()
        .map(|(e, s)| (e.as_str(), s.descriptor.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [("ahu-01", "temp"), ("ahu-02", "a"), ("ahu-02", "b")]
    );
    let after = Some(("ahu-02".to_owned(), "a".to_owned()));
    assert_eq!(r.list_signals(Some("ahu-02"), after, 10, |_| true).len(), 1);
}

#[test]
fn a_full_collection_refuses_with_its_limit() {
    let dir = Dir::new();
    let mut limits = Limits::default();
    limits.max_streams = 1;
    let (r, _) = Registry::open(&dir.0, limits, MetadataLimits::default()).unwrap();
    r.create_stream("a", StreamSpec::default(), t(1), Write::Commit)
        .unwrap();
    let e = r.create_stream("b", StreamSpec::default(), t(1), Write::Commit);
    assert!(matches!(
        e,
        Err(MetadataError::LimitExceeded {
            limit: "max_streams",
            ..
        })
    ));
}
