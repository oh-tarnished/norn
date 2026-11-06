//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use norn_protocol::entity::Entity;
use norn_protocol::signal::Signal;

fn entity(n: usize) -> Resource {
    Resource::Entity(Entity {
        name: format!("entities/d{n}"),
        ..Entity::default()
    })
}

fn signal(n: usize, unit: &str) -> Resource {
    Resource::Signal {
        parent: "entities/d1".to_owned(),
        id: format!("s{n}"),
        signal: Signal {
            name: format!("entities/d1/signals/s{n}"),
            unit: unit.to_owned(),
            ..Signal::default()
        },
    }
}

#[test]
fn entities_go_before_signals_whatever_order_they_were_declared_in() {
    let mut work = Work::default();
    for n in 0..3 {
        work.push(entity(n));
        work.push(signal(n, ""));
    }
    assert_eq!(
        work.take(1000).map(|(k, t)| (k, t.len())),
        Some((Kind::Entities, 3))
    );
    assert_eq!(
        work.take(1000).map(|(k, t)| (k, t.len())),
        Some((Kind::Signals, 3))
    );
    assert!(work.take(1000).is_none());
}

#[test]
fn a_signal_declared_again_while_queued_is_sent_once_with_the_newest() {
    let mut work = Work::default();
    work.push(signal(1, ""));
    work.push(signal(2, ""));
    work.push(signal(1, "degrees-celsius"));
    let (_, taken) = work.take(1000).unwrap();
    assert_eq!(taken.len(), 2, "coalesced");
    let Resource::Signal { signal, .. } = &taken[0] else {
        panic!("a signal");
    };
    assert_eq!(
        (signal.name.as_str(), signal.unit.as_str()),
        ("entities/d1/signals/s1", "degrees-celsius")
    );
}

#[test]
fn updates_wait_for_every_create_and_a_chunk_put_back_keeps_its_place() {
    let mut work = Work::default();
    work.update(signal(9, "x"));
    work.push(signal(1, ""));
    work.push(signal(2, ""));
    assert_eq!(work.next(), Some(Kind::Signals));
    let (kind, taken) = work.take(1).unwrap();
    work.put_back(kind, taken);
    let (_, again) = work.take(1000).unwrap();
    let names: Vec<&str> = again.iter().map(name).collect();
    assert_eq!(names, ["entities/d1/signals/s1", "entities/d1/signals/s2"]);
    assert_eq!(work.creates(), 0);
    assert_eq!(work.take(1000).map(|(k, _)| k), Some(Kind::Updates));
    assert!(work.is_empty());
}
