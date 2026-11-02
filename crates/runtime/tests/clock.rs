//! The injected clocks: a deadline is a time, so an advance is never lost.
//!
//! The replay driver reads the clock, asks the session how long to wait, then
//! sleeps. An advance landing between the reading and the sleep once hung
//! `real_time_follows_the_injected_clock`: a relative sleep measured from after
//! the advance, so its deadline moved past the advance that should have ended it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use norn_core::Timestamp;
use norn_runtime::{Clock, FakeClock, SystemClock};

const BOUND: Duration = Duration::from_secs(5);

fn plus(t: Timestamp, nanos: i64) -> Timestamp {
    Timestamp::from_nanos(t.as_nanos() + nanos)
}

#[tokio::test]
async fn an_advance_before_a_deadline_sleep_starts_is_not_lost() {
    let clock = FakeClock::new(Timestamp::UNIX_EPOCH);
    let wall = clock.now();
    clock.advance(Duration::from_nanos(4));
    // A wait of 1 ns measured from `wall`, begun after the advance: already over.
    let slept = tokio::time::timeout(BOUND, clock.sleep_until(plus(wall, 1))).await;
    assert!(slept.is_ok(), "the advance before the sleep was lost");
}

#[tokio::test]
async fn a_relative_sleep_counts_from_when_it_is_called() {
    // Why a wait computed from an earlier reading must not use `sleep`.
    let clock = FakeClock::new(Timestamp::UNIX_EPOCH);
    clock.advance(Duration::from_nanos(4));
    let mut sleep = clock.sleep(Duration::from_nanos(1));
    let early = tokio::time::timeout(Duration::from_millis(20), &mut sleep).await;
    assert!(early.is_err(), "a relative sleep ended with no advance");
    clock.advance(Duration::from_nanos(1));
    assert!(tokio::time::timeout(BOUND, sleep).await.is_ok());
}

#[tokio::test]
async fn a_deadline_sleep_waits_for_its_deadline_and_no_longer() {
    let clock = FakeClock::new(Timestamp::UNIX_EPOCH);
    let mut sleep = clock.sleep_until(plus(clock.now(), 10));
    clock.advance(Duration::from_nanos(9));
    let early = tokio::time::timeout(Duration::from_millis(20), &mut sleep).await;
    assert!(early.is_err(), "ended a nanosecond early");
    clock.advance(Duration::from_nanos(1));
    assert!(tokio::time::timeout(BOUND, sleep).await.is_ok());
}

#[tokio::test]
async fn a_past_deadline_on_the_system_clock_ends_at_once() {
    let clock = SystemClock;
    let past = plus(clock.now(), -1_000_000_000);
    assert!(
        tokio::time::timeout(BOUND, clock.sleep_until(past))
            .await
            .is_ok()
    );
}
