//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What comes after something went wrong: calls lost, limits met.

use super::{O, Outcome, R, T0, first, flow, samples, second};
use crate::health::Notice;

#[test]
fn a_lost_call_resends_what_was_unanswered_with_batch_ids_from_one() {
    let mut f = flow(10);
    f.offer(samples(2));
    let sent = f.take(T0).unwrap();
    f.offer(samples(1));
    f.take(T0).unwrap();
    f.lost();
    let again = f.take(T0).unwrap();
    assert_eq!(again.batch_id, 1);
    assert_eq!(again.samples[..2], sent.samples[..]);
    assert_eq!(again.samples.len(), 3);
}

#[test]
fn a_missing_stream_asks_for_a_new_call_and_loses_nothing() {
    let mut f = flow(10);
    f.offer(samples(1));
    f.take(T0).unwrap();
    let mut reply = first(1, O::Reject, R::Unspecified, None);
    reply.error = Some(norn_protocol::rpc::pb::Status {
        code: 5,
        ..Default::default()
    });
    assert!(matches!(f.answer(&reply, T0), Outcome::Reconnect(_)));
    assert_eq!(f.held(), 1);
}

#[test]
fn a_batch_that_cannot_reach_its_level_is_sent_again() {
    let mut f = flow(10);
    f.offer(samples(1));
    f.take(T0).unwrap();
    f.answer(&first(1, O::Accept, R::CapacityAvailable, None), T0);
    let mut late = second(1);
    late.ack_level = 0;
    late.error = Some(norn_protocol::rpc::pb::Status::default());
    assert!(matches!(
        f.answer(&late, T0),
        Outcome::Tell(Notice::StoppedShort { .. })
    ));
    assert_eq!((f.held(), f.counts().acknowledged), (1, 0));
}

#[test]
fn the_window_stops_at_the_limit_a_daemon_published() {
    let mut f = flow(100);
    f.limit(1, 1 << 20, 2);
    f.offer(samples(5));
    assert!(f.take(T0).is_some());
    assert!(f.take(T0).is_some());
    assert!(f.take(T0).is_none(), "two batches is the window");
}

#[test]
fn the_byte_limit_splits_a_batch_but_never_starves_a_lone_sample() {
    let mut f = flow(100);
    f.limit(1000, 40, 16);
    f.offer(samples(10));
    let n = f.take(T0).unwrap().samples.len();
    assert!((1..10).contains(&n), "{n}");
}
