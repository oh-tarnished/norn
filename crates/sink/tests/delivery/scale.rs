//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A cold start at scale: 20,000 signals declared at once while readings keep
//! coming, against a registry that creates as Norn does. Readings of the signals
//! already made must go while the rest are created, none may arrive before its
//! own signal, and the 256 KiB message limit is never passed.

use crate::daemon::Daemon;
use crate::fake::Log;
use crate::registry::Registry;
use crate::support::{Socket, acknowledged, run};
use norn_sink::{Catalog, Entity, Kind, Options, Reading, Signal, Sink, ValueType};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Entities, and signals on each: 20,000 signals.
const ENTITIES: usize = 200;
const SIGNALS: usize = 100;

/// What a large deployment says of each signal, so that a batch of 1,000 is past
/// the message limit.
fn described() -> Catalog {
    let mut catalog = Catalog::new();
    for entity in 0..ENTITIES {
        let id = format!("machine-{entity}");
        catalog.push_entity(Entity::new(&id).display_name(format!("Machine {entity}")));
        for signal in 0..SIGNALS {
            catalog.push_signal(
                Signal::new(&id, format!("s{signal}"), ValueType::Double)
                    .display_name(format!("Machine {entity} line {signal} outlet temperature"))
                    .unit("degree_celsius")
                    .annotation(
                        "description",
                        format!(
                            "Outlet temperature after the second stage, line {signal} on machine \
                             {entity}, as the source reports it, with its long free text"
                        ),
                    )
                    .annotation("source.address", format!("10.0.{entity}.{signal}")),
            );
        }
    }
    catalog
}

/// One reading of every signal.
fn sweep(round: i64) -> Vec<Reading> {
    let mut readings = Vec::with_capacity(ENTITIES * SIGNALS);
    for entity in 0..ENTITIES {
        for signal in 0..SIGNALS {
            let at = prost_types::Timestamp {
                seconds: 1_000_000 + round,
                nanos: 0,
            };
            readings.push(Reading::new(
                &format!("machine-{entity}"),
                &format!("s{signal}"),
                at,
                Kind::DoubleValue(1.0),
            ));
        }
    }
    readings
}

#[tokio::test(flavor = "multi_thread")]
async fn twenty_thousand_signals_are_made_in_seconds_and_readings_flow_meanwhile() {
    let socket = Socket::new("scale");
    let (log, registry) = (Log::default(), Registry::default());
    let daemon = Daemon::with_registry(&socket.endpoint(), log.clone(), registry.clone()).await;
    let options = Options::default().buffer_bytes(512 << 20);
    let sink = Sink::new(socket.endpoint(), "plant", options).unwrap();
    let start = Instant::now();
    sink.declare(&described()).unwrap();
    sink.push(sweep(0)).unwrap();
    assert_eq!(
        sink.health().unmade,
        ENTITIES * SIGNALS,
        "every signal declared, none made"
    );
    let running = run(sink);
    let (mut flowed_early, mut rounds) = (false, 1);
    while running.sink.health().unmade > 0 {
        let left = running.sink.health().unmade;
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "stalled: {left} left"
        );
        flowed_early |= running.sink.counts().acknowledged > 0;
        if start.elapsed() > Duration::from_millis(100 * rounds) {
            // The next round of readings, as a source keeps sending them.
            running
                .sink
                .push(sweep(i64::try_from(rounds).unwrap()))
                .unwrap();
            rounds += 1;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(
        flowed_early,
        "readings of made signals went while the rest were created"
    );
    // Cut to the message limit, not one at a time: the registry refuses a batch
    // past 256 KiB, and a create of one signal.
    assert_eq!(
        registry.past_limit.load(Ordering::SeqCst),
        0,
        "never past the limit"
    );
    assert_eq!(
        registry.singly.load(Ordering::SeqCst),
        0,
        "never one at a time"
    );
    let largest = registry.largest.load(Ordering::SeqCst);
    assert!(
        largest >= 300,
        "batches as full as the limit allows: {largest}"
    );
    let batches = registry.batches.load(Ordering::SeqCst);
    assert!(batches <= 80, "{batches} batches for 20,000 signals");
    let total = u64::try_from(ENTITIES * SIGNALS).unwrap() * rounds;
    acknowledged(&running.sink, total).await;
    assert_eq!(
        log.lock().unwrap().early,
        0,
        "no reading before its own signal"
    );
    let report = running.close().await;
    assert_eq!(report.counts.acknowledged, total);
    assert_eq!(report.counts.dropped(), 0);
    daemon.stop().await;
}
