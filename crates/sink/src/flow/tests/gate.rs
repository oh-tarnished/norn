//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The readiness gate: a reading waits for its own signal, and only for it.

use super::{T0, flow, samples};

#[test]
fn a_reading_of_an_unmade_signal_waits_while_the_others_go() {
    let mut f = flow(10);
    f.block("entities/e/signals/s1");
    f.offer(samples(3));
    assert_eq!(f.unmade(), 1);
    let batch = f.take(T0).unwrap();
    let sent: Vec<i64> = batch.samples.iter().map(|s| s.sequence).collect();
    assert_eq!(sent, [0, 2], "s1 waits; s0 and s2 go");
    assert_eq!(f.held(), 3, "the waiting one is still held");
    assert!(f.take(T0).is_none(), "nothing else is ready");
}

#[test]
fn a_signal_made_lets_its_readings_go_in_their_order() {
    let mut f = flow(10);
    f.block("entities/e/signals/s1");
    let mut two = samples(2);
    two.extend(samples(2));
    f.offer(two);
    f.take(T0).unwrap();
    f.made(&["entities/e/signals/s1".to_owned()]);
    assert_eq!(f.unmade(), 0);
    let batch = f.take(T0).unwrap();
    let signals: Vec<&str> = batch.samples.iter().map(|s| s.signal.as_str()).collect();
    assert_eq!(signals, ["entities/e/signals/s1", "entities/e/signals/s1"]);
}

#[test]
fn a_full_buffer_gives_up_waiting_readings_once_nothing_ready_is_left() {
    let mut f = flow(2);
    let bound = f.capacity;
    for i in 0..3 {
        f.block(&format!("entities/e/signals/s{i}"));
    }
    let gone = f.offer(samples(3));
    assert!(gone >= 1, "past the bound, waiting or not");
    assert!(f.held_bytes() <= bound);
    assert_eq!(f.held() + gone, 3, "every reading held or counted");
    assert_eq!(f.counts().overflowed, gone as u64);
    assert_eq!(f.abandon(), 3 - gone, "the rest given up as the run ends");
    assert_eq!(f.held_bytes(), 0);
}
