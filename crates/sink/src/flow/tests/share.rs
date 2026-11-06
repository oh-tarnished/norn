//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The buffer's two bounds: bytes in all, and a share for each signal.

use super::{T0, samples};
use crate::flow::Flow;
use crate::flow::hold::cost;
use norn_protocol::common::Sample;
use norn_protocol::common::Value;
use norn_protocol::common::value::Kind;
use norn_protocol::telemetry::AcknowledgementLevel as Ack;

/// `n` samples of the one signal `chatty`, each costing the same.
fn storm(n: usize) -> Vec<Sample> {
    let one = Sample {
        signal: "entities/e/signals/chatty".into(),
        sequence: 7,
        ..Sample::default()
    };
    vec![one; n]
}

#[test]
fn one_chatty_signal_is_gated_at_its_share_and_the_others_still_fit() {
    let mut f = Flow::new("streams/s".into(), Ack::Persisted, 64 << 20);
    let share = f.share();
    let each = cost(&storm(1)[0]);
    let gone = f.offer(storm(share / each * 4));
    assert!(gone > 0, "a storm past its share is cut");
    assert_eq!(f.counts().gated, gone as u64);
    assert_eq!(f.counts().overflowed, 0, "the buffer itself never filled");
    assert!(
        f.held_bytes() <= share,
        "one signal holds its share at most"
    );
    assert_eq!(f.offer(samples(10)), 0, "every other signal still has room");
    assert_eq!(f.held(), share / each + 10);
}

#[test]
fn the_bound_is_bytes_so_long_text_is_held_by_its_size_not_its_count() {
    let text = |i: usize| Sample {
        signal: format!("entities/e/signals/t{i}"),
        value: Some(Value {
            kind: Some(Kind::StringValue("x".repeat(4000))),
        }),
        ..Sample::default()
    };
    let small: usize = samples(100).iter().map(cost).sum();
    let mut f = Flow::new("streams/s".into(), Ack::Persisted, small);
    assert_eq!(f.offer(samples(100)), 0, "a hundred numbers fit");
    let mut f2 = Flow::new("streams/s".into(), Ack::Persisted, small);
    let gone = f2.offer((0..100).map(text).collect());
    assert!(gone > 90, "a hundred long texts do not: {gone} given up");
    assert!(f2.held_bytes() <= small);
    assert_eq!(f2.counts().overflowed, gone as u64);
    f.take(T0).unwrap();
}

#[test]
fn what_is_acknowledged_or_abandoned_frees_its_bytes() {
    let mut f = super::flow(10);
    f.offer(samples(3));
    let held = f.held_bytes();
    assert!(held > 0);
    f.take(T0).unwrap();
    f.answer(&super::second(1), T0);
    assert_eq!(f.held_bytes(), 0, "acknowledged");
    f.offer(samples(3));
    f.abandon();
    assert_eq!(f.held_bytes(), 0, "abandoned");
}
