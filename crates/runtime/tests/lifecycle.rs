//! The runtime lifecycle, on a fake clock.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use norn_core::{Config, Timestamp};
use norn_runtime::{FakeClock, Health, Runtime, RuntimeError};
use tokio_util::sync::CancellationToken;

fn fake() -> (FakeClock, Runtime) {
    let clock = FakeClock::new(Timestamp::from_nanos(1_000));
    let runtime = Runtime::with_clock(Config::default(), Arc::new(clock.clone())).unwrap();
    (clock, runtime)
}

#[tokio::test]
async fn new_spawns_nothing_and_reports_starting() {
    let started = Arc::new(AtomicUsize::new(0));
    let (_, runtime) = fake();
    let seen = Arc::clone(&started);
    runtime
        .add_task("counter", move |_| async move {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.health().get(), Health::Starting);
    assert_eq!(runtime.start_time(), Timestamp::from_nanos(1_000));
    drop(runtime);
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn close_is_idempotent_and_a_closed_runtime_does_not_run() {
    let (_, runtime) = fake();
    runtime.close();
    runtime.close();
    assert_eq!(runtime.health().get(), Health::Stopped);
    let err = runtime.run(CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err, RuntimeError::Closed));
    runtime.close();
}

#[tokio::test]
async fn run_stops_its_tasks_before_returning() {
    let (_, runtime) = fake();
    let stopped = Arc::new(AtomicUsize::new(0));
    for _ in 0..3 {
        let stopped = Arc::clone(&stopped);
        runtime
            .add_task("worker", move |token| async move {
                token.cancelled().await;
                stopped.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
    }
    let cancel = CancellationToken::new();
    let mut health = runtime.health();
    let run = runtime.run(cancel.clone());
    tokio::pin!(run);
    tokio::select! {
        _ = &mut run => panic!("run returned before cancellation"),
        h = health.reached(Health::Serving) => assert_eq!(h, Health::Serving),
    }
    cancel.cancel();
    let report = run.await.unwrap();
    assert_eq!(report.aborted, 0);
    assert_eq!(stopped.load(Ordering::SeqCst), 3);
    assert_eq!(runtime.health().get(), Health::Stopped);
}

#[tokio::test]
async fn a_task_ignoring_cancellation_is_aborted_at_the_shutdown_timeout() {
    let (clock, runtime) = fake();
    let runtime = runtime.shutdown_timeout(Duration::from_secs(5));
    runtime
        .add_task("stubborn", |_| std::future::pending())
        .unwrap();
    let runtime = Arc::new(runtime);
    let cancel = CancellationToken::new();
    let running = {
        let runtime = Arc::clone(&runtime);
        let cancel = cancel.clone();
        tokio::spawn(async move { runtime.run(cancel).await })
    };
    let mut health = runtime.health();
    health.reached(Health::Serving).await;
    cancel.cancel();
    assert_eq!(health.reached(Health::Draining).await, Health::Draining);
    while clock.sleepers() == 0 {
        tokio::task::yield_now().await;
    }
    clock.advance(Duration::from_secs(4));
    tokio::task::yield_now().await;
    assert!(!running.is_finished());
    clock.advance(Duration::from_secs(1));
    let report = running.await.unwrap().unwrap();
    assert_eq!(report.aborted, 1);
    assert_eq!(runtime.health().get(), Health::Stopped);
}

#[tokio::test]
async fn a_failing_task_stops_the_others_and_is_reported() {
    let (_, runtime) = fake();
    let stopped = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&stopped);
    runtime
        .add_task("patient", move |token| async move {
            token.cancelled().await;
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    runtime
        .add_task("broken", |_| async { Err("disk on fire".into()) })
        .unwrap();
    let err = runtime.run(CancellationToken::new()).await.unwrap_err();
    assert!(matches!(
        err,
        RuntimeError::TaskFailed { task: "broken", .. }
    ));
    assert_eq!(stopped.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.health().get(), Health::Stopped);
    runtime.close();
}

#[tokio::test]
async fn a_runtime_runs_once_and_refuses_tasks_after_starting() {
    let (_, runtime) = fake();
    runtime.close();
    let (_, runtime2) = fake();
    let cancel = CancellationToken::new();
    cancel.cancel();
    runtime2.run(cancel.clone()).await.unwrap();
    let again = runtime2.run(cancel).await.unwrap_err();
    assert!(matches!(again, RuntimeError::AlreadyStarted));
    let late = runtime2.add_task("late", |_| async { Ok(()) });
    assert!(matches!(late, Err(RuntimeError::AlreadyStarted)));
}

#[tokio::test]
async fn close_during_run_drains_like_cancellation() {
    let (_, runtime) = fake();
    runtime
        .add_task("worker", |token| async move {
            token.cancelled().await;
            Ok(())
        })
        .unwrap();
    let runtime = Arc::new(runtime);
    let running = {
        let runtime = Arc::clone(&runtime);
        tokio::spawn(async move { runtime.run(CancellationToken::new()).await })
    };
    runtime.health().reached(Health::Serving).await;
    runtime.close();
    let report = running.await.unwrap().unwrap();
    assert_eq!(report.aborted, 0);
    runtime.close();
}
