//! Memory budget reservations, refusals and bounded waiting.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use norn_core::{Config, Timestamp};
use norn_runtime::{Budget, BudgetError, Clock, FakeClock, Runtime, Subsystem};

fn budget(capacity: u64) -> (FakeClock, Budget) {
    let clock = FakeClock::new(Timestamp::UNIX_EPOCH);
    let shared: Arc<dyn Clock> = Arc::new(clock.clone());
    (clock, Budget::new("test_bytes", capacity, shared))
}

#[test]
fn a_reservation_within_capacity_is_granted_and_released_on_drop() {
    let (_, b) = budget(100);
    let r = b.try_reserve(60).unwrap();
    assert_eq!((r.bytes(), b.used(), b.available()), (60, 60, 40));
    drop(r);
    assert_eq!(b.used(), 0);
}

#[test]
fn a_reservation_past_what_is_free_is_refused_with_the_shortfall() {
    let (_, b) = budget(100);
    let _held = b.try_reserve(70).unwrap();
    let err = b.try_reserve(40).unwrap_err();
    assert_eq!(
        err,
        BudgetError::Exhausted {
            budget: "test_bytes",
            requested: 40,
            available: 30
        }
    );
    assert_eq!(b.used(), 70, "a refusal reserves nothing");
}

#[test]
fn a_reservation_larger_than_the_whole_budget_is_too_large_not_exhausted() {
    let (_, b) = budget(100);
    let err = b.try_reserve(101).unwrap_err();
    assert!(matches!(err, BudgetError::TooLarge { capacity: 100, .. }));
}

#[test]
fn shrinking_returns_the_difference_and_never_grows() {
    let (_, b) = budget(100);
    let mut r = b.try_reserve(80).unwrap();
    r.shrink_to(30);
    assert_eq!((r.bytes(), b.used()), (30, 30));
    r.shrink_to(90);
    assert_eq!((r.bytes(), b.used()), (30, 30));
}

#[tokio::test]
async fn a_waiting_reservation_is_granted_when_another_is_released() {
    let (_, b) = budget(100);
    let held = b.try_reserve(100).unwrap();
    let waiter = {
        let b = b.clone();
        tokio::spawn(async move { b.reserve(50, Duration::from_secs(1)).await })
    };
    tokio::task::yield_now().await;
    drop(held);
    let granted = waiter.await.unwrap().unwrap();
    assert_eq!((granted.bytes(), b.used()), (50, 50));
}

#[tokio::test]
async fn a_waiting_reservation_times_out_at_its_bound_on_the_clock() {
    let (clock, b) = budget(100);
    let _held = b.try_reserve(90).unwrap();
    let waiter = {
        let b = b.clone();
        tokio::spawn(async move { b.reserve(50, Duration::from_millis(200)).await })
    };
    while clock.sleepers() == 0 {
        tokio::task::yield_now().await;
    }
    clock.advance(Duration::from_millis(200));
    let err = waiter.await.unwrap().unwrap_err();
    assert_eq!(
        err,
        BudgetError::TimedOut {
            budget: "test_bytes",
            requested: 50,
            available: 10,
            waited: Duration::from_millis(200)
        }
    );
}

#[tokio::test]
async fn closing_a_budget_wakes_waiters_and_refuses_new_reservations() {
    let (_, b) = budget(10);
    let held = b.try_reserve(10).unwrap();
    let waiter = {
        let b = b.clone();
        tokio::spawn(async move { b.reserve(5, Duration::from_secs(60)).await })
    };
    tokio::task::yield_now().await;
    b.close();
    let err = waiter.await.unwrap().unwrap_err();
    assert_eq!(
        err,
        BudgetError::Closed {
            budget: "test_bytes"
        }
    );
    assert!(matches!(b.try_reserve(1), Err(BudgetError::Closed { .. })));
    drop(held);
    assert_eq!(b.used(), 0, "held bytes still return after close");
}

#[test]
fn the_runtime_draws_one_budget_per_subsystem_from_the_config() {
    let memory = Config::default().memory();
    let runtime = Runtime::new(Config::default()).unwrap();
    let ingest = runtime.budgets().get(Subsystem::Ingest);
    assert_eq!(ingest.capacity(), memory.ingest_bytes);
    assert_eq!(ingest.name(), "ingest_bytes");
    let sum: u64 = Subsystem::ALL
        .iter()
        .map(|s| runtime.budgets().get(*s).capacity())
        .sum();
    assert!(sum <= memory.total_bytes);
}
