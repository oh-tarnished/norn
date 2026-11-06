//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn away, going away and draining: nothing lost, nothing stored twice.

use crate::daemon::Daemon;
use crate::fake::Log;
use crate::support::{Socket, acknowledged, holding, readings, started};
use norn_sink::{Options, Reading};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[tokio::test]
async fn a_norn_away_at_start_is_waited_for_and_loses_nothing() {
    let socket = Socket::new("late");
    let run = holding(socket.endpoint(), 1 << 20);
    let reach = run.sink.reach();
    run.sink.push(readings(0, 3)).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!reach.borrow().up);
    assert!(reach.borrow().detail.contains("unreachable"));
    assert_eq!(run.sink.health().held, 3);
    let log = Log::default();
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    acknowledged(&run.sink, 3).await;
    assert!(reach.borrow().up);
    assert_eq!(log.lock().unwrap().stored, [1, 2, 3]);
    run.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn a_norn_that_goes_away_is_reported_and_what_comes_after_still_arrives() {
    let socket = Socket::new("gone");
    let log = Log::default();
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = holding(socket.endpoint(), 1 << 20);
    let reach = run.sink.reach();
    run.sink.push(readings(0, 2)).unwrap();
    acknowledged(&run.sink, 2).await;
    daemon.stop().await;
    run.sink.push(readings(2, 2)).unwrap();
    let start = Instant::now();
    while reach.borrow().up {
        assert!(start.elapsed() < Duration::from_secs(10), "never noticed");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    acknowledged(&run.sink, 4).await;
    assert!(reach.borrow().up);
    assert_eq!(log.lock().unwrap().stored.len(), 4);
    run.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn a_call_dying_before_its_answer_resends_and_norn_stores_each_once() {
    let socket = Socket::new("resend");
    let log = Log::default();
    log.lock().unwrap().die_after = Some(1);
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = holding(socket.endpoint(), 1 << 20);
    run.sink.push(readings(0, 2)).unwrap();
    acknowledged(&run.sink, 2).await;
    // The second batch reaches the fake, is stored, and its answer never comes.
    run.sink.push(readings(2, 2)).unwrap();
    acknowledged(&run.sink, 4).await;
    {
        let seen = log.lock().unwrap();
        assert!(seen.die_after.is_none(), "the call died");
        assert_eq!(seen.arrived, [1, 2, 3, 4, 3, 4], "resent, unchanged");
        assert_eq!(seen.stored, [1, 2, 3, 4], "stored once");
    }
    let report = run.close().await;
    assert_eq!(report.counts.acknowledged, 4);
    assert_eq!(
        report.counts.duplicates, 2,
        "Norn said which it already had"
    );
    assert_eq!(report.never_sent, 0);
    daemon.stop().await;
}

#[tokio::test]
async fn a_drain_ends_the_call_and_the_sink_resends_on_a_new_one_still_healthy() {
    let socket = Socket::new("drain");
    let log = Log::default();
    log.lock().unwrap().drain_after = Some(1);
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let told = Arc::new(AtomicBool::new(false));
    let saw = Arc::clone(&told);
    let options = Options::default().on_notice(move |notice| {
        if matches!(notice, norn_sink::Notice::Draining { .. }) {
            saw.store(true, Ordering::SeqCst);
        }
    });
    let run = started(socket.endpoint(), options);
    run.sink.push(readings(0, 2)).unwrap();
    acknowledged(&run.sink, 2).await;
    // Every change of reach the sink makes from here on.
    let mut reach = run.sink.reach();
    let down = Arc::new(AtomicBool::new(false));
    let went = Arc::clone(&down);
    let watching = tokio::spawn(async move {
        while reach.changed().await.is_ok() {
            if !reach.borrow().up {
                went.store(true, Ordering::SeqCst);
            }
        }
    });
    run.sink.push(readings(2, 2)).unwrap();
    acknowledged(&run.sink, 4).await;
    {
        let seen = log.lock().unwrap();
        assert!(seen.drain_after.is_none(), "the drain happened");
        assert_eq!(seen.stored, [1, 2, 3, 4], "resent, once");
    }
    // Watched up to the stop: a stopped daemon is a real absence.
    watching.abort();
    assert!(
        !down.load(Ordering::SeqCst),
        "a drain is not Norn unreachable"
    );
    assert!(told.load(Ordering::SeqCst), "and it is told");
    run.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn close_with_norn_away_gives_up_after_the_drain_and_says_how_many() {
    let socket = Socket::new("away");
    let drain = Duration::from_millis(100);
    let run = started(socket.endpoint(), Options::default().drain(drain));
    let some: Vec<Reading> = readings(0, 3);
    run.sink.push(some).unwrap();
    let start = Instant::now();
    let report = run.close().await;
    assert!(start.elapsed() < Duration::from_secs(5), "bounded");
    assert_eq!(report.never_sent, 3);
    assert_eq!(report.counts.acknowledged, 0);
}

#[tokio::test]
async fn a_run_that_starts_after_close_still_drains_what_is_held() {
    let socket = Socket::new("racing");
    let log = Log::default();
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = crate::support::started(socket.endpoint(), Options::default());
    let sink = run.sink.clone();
    sink.push(readings(0, 3)).unwrap();
    // Before the spawned run has had a turn, on this one thread.
    sink.close().await;
    let report = run.close().await;
    assert_eq!(report.counts.acknowledged, 3);
    assert_eq!(log.lock().unwrap().stored, [1, 2, 3]);
    daemon.stop().await;
}

#[tokio::test]
async fn close_with_no_run_at_all_gives_up_after_the_drain_and_tells() {
    let told = Arc::new(std::sync::Mutex::new(Vec::new()));
    let keep = Arc::clone(&told);
    let options = Options::default()
        .drain(Duration::from_millis(20))
        .on_notice(move |n| keep.lock().unwrap().push(n.clone()));
    let sink = norn_sink::Sink::new("unix:/n/n.sock".parse().unwrap(), "plant", options).unwrap();
    let catalog = norn_sink::Catalog::new()
        .entity(norn_sink::Entity::new("e"))
        .signal(norn_sink::Signal::new(
            "e",
            "s",
            norn_sink::ValueType::Double,
        ));
    sink.declare(&catalog).unwrap();
    sink.push(readings(0, 2)).unwrap();
    sink.close().await;
    let said = norn_sink::Notice::NeverSent { readings: 2 };
    assert_eq!(*told.lock().unwrap(), [said]);
    assert_eq!(sink.health().held, 0);
}
