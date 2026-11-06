//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Batch provisioning: cut to the message limit, halved when a limit is wrong,
//! one at a time where a daemon has no batch create.

use crate::daemon::Daemon;
use crate::fake::Log;
use crate::registry::{LIMIT, Registry};
use crate::support::{Socket, acknowledged, holding, readings, run};
use norn_sink::{Catalog, Entity, Options, Signal, Sink, ValueType};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// `n` signals, each about 500 bytes on the wire.
fn large(n: usize) -> Catalog {
    let mut catalog = Catalog::new().entity(Entity::new("e"));
    for i in 0..n {
        catalog.push_signal(
            Signal::new("e", format!("s{i}"), ValueType::Double)
                .display_name(format!("Signal {i}: ").repeat(40)),
        );
    }
    catalog
}

async fn all_made(sink: &Sink) {
    let start = Instant::now();
    while sink.health().unmade > 0 {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{:?}",
            sink.health()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn a_daemon_publishing_no_limit_is_cut_to_the_known_one_and_never_passed() {
    let socket = Socket::new("known");
    let registry = Registry::default();
    let daemon = Daemon::with_registry(&socket.endpoint(), Log::default(), registry.clone()).await;
    let sink = Sink::new(socket.endpoint(), "plant", Options::default()).unwrap();
    sink.declare(&large(1_000)).unwrap();
    let running = run(sink);
    all_made(&running.sink).await;
    assert_eq!(
        registry.past_limit.load(Ordering::SeqCst),
        0,
        "never refused"
    );
    assert_eq!(
        registry.singly.load(Ordering::SeqCst),
        0,
        "never one at a time"
    );
    assert!(registry.batches.load(Ordering::SeqCst) >= 2, "cut");
    assert_eq!(registry.made().lock().unwrap().len(), 1_000);
    running.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn a_limit_guessed_too_high_is_halved_on_resource_exhausted_and_all_lands() {
    let socket = Socket::new("halve");
    let registry = Registry::default();
    let daemon = Daemon::with_registry(&socket.endpoint(), Log::default(), registry.clone()).await;
    let wrong = Options::default().unary_limit(4 * LIMIT * 4);
    let sink = Sink::new(socket.endpoint(), "plant", wrong).unwrap();
    sink.declare(&large(1_000)).unwrap();
    let running = run(sink);
    all_made(&running.sink).await;
    assert!(
        registry.past_limit.load(Ordering::SeqCst) > 0,
        "it was refused"
    );
    assert_eq!(
        registry.singly.load(Ordering::SeqCst),
        0,
        "halves, not one by one"
    );
    assert_eq!(registry.made().lock().unwrap().len(), 1_000);
    running.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn a_daemon_with_no_batch_create_gets_them_one_at_a_time() {
    let socket = Socket::new("single");
    let log = Log::default();
    // `Daemon::start`'s stubs answer batch create `UNIMPLEMENTED`.
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let run = holding(socket.endpoint(), 1 << 20);
    run.sink.push(readings(0, 2)).unwrap();
    acknowledged(&run.sink, 2).await;
    assert_eq!(run.sink.health().unmade, 0);
    assert_eq!(log.lock().unwrap().stored, [1, 2]);
    run.close().await;
    daemon.stop().await;
}
