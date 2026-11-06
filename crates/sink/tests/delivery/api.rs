//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the adapter's calls promise: typed errors, nothing changed by one, `new`
//! spawning nothing, `close` idempotent.

use crate::daemon::Daemon;
use crate::fake::Log;
use crate::support::{Socket, acknowledged, readings, run, started};
use norn_sink::{
    Catalog, Entity, Kind, Notice, Options, Reading, Signal, Sink, SinkError, ValueType,
};
use std::sync::{Arc, Mutex};

fn sink() -> Sink {
    let endpoint = "unix:/nonexistent/n.sock".parse().unwrap();
    Sink::new(endpoint, "plant", Options::default()).unwrap()
}

#[tokio::test]
async fn new_starts_nothing_and_close_with_nothing_written_returns_twice() {
    let sink = sink();
    sink.close().await;
    sink.close().await;
    assert_eq!(sink.run().await, Err(SinkError::Closed));
    assert_eq!(sink.push(readings(0, 1)), Err(SinkError::Closed));
}

#[tokio::test]
async fn a_second_run_is_refused_and_close_ends_the_first() {
    let socket = Socket::new("twice");
    let running = started(socket.endpoint(), Options::default());
    tokio::task::yield_now().await;
    let sink = running.sink.clone();
    while sink.run().await != Err(SinkError::AlreadyRunning) {
        tokio::task::yield_now().await;
    }
    running.close().await;
    assert_eq!(sink.run().await, Err(SinkError::Closed));
}

#[test]
fn a_stream_id_norn_would_refuse_is_refused_at_new() {
    let endpoint = "unix:/n.sock".parse().unwrap();
    let error = Sink::new(endpoint, "Not An Id", Options::default()).unwrap_err();
    assert!(matches!(
        error,
        SinkError::InvalidId {
            field: "stream_id",
            ..
        }
    ));
}

#[test]
fn a_catalog_with_a_mistake_declares_nothing_of_itself() {
    let sink = sink();
    let good = Signal::new("e", "good", ValueType::Double);
    let orphan = Catalog::new().signal(good.clone());
    assert!(matches!(
        sink.declare(&orphan),
        Err(SinkError::UnknownEntity { .. })
    ));
    let bad_id = Catalog::new()
        .entity(Entity::new("e"))
        .signal(good.clone())
        .signal(Signal::new("e", "Bad", ValueType::Double));
    assert!(matches!(
        sink.declare(&bad_id),
        Err(SinkError::InvalidId {
            field: "signal_id",
            ..
        })
    ));
    assert_eq!(sink.health().unmade, 0, "not even the good one");
    let catalog = Catalog::new().entity(Entity::new("e")).signal(good);
    let first = sink.declare(&catalog).unwrap();
    assert_eq!((first.entities, first.signals), (1, 1));
    let again = sink.declare(&catalog).unwrap();
    assert_eq!(
        (again.entities, again.signals),
        (0, 0),
        "unchanged costs nothing"
    );
    let retyped = Catalog::new().signal(Signal::new("e", "good", ValueType::Int64));
    assert!(matches!(
        sink.declare(&retyped),
        Err(SinkError::TypeChanged { .. })
    ));
}

#[test]
fn too_many_signals_is_refused_whole() {
    let endpoint = "unix:/n.sock".parse().unwrap();
    let sink = Sink::new(endpoint, "plant", Options::default().max_signals(1)).unwrap();
    let catalog = Catalog::new()
        .entity(Entity::new("e"))
        .signal(Signal::new("e", "a", ValueType::Double))
        .signal(Signal::new("e", "b", ValueType::Double));
    assert_eq!(
        sink.declare(&catalog),
        Err(SinkError::TooManySignals { limit: 1 })
    );
}

#[test]
fn a_reading_of_an_undeclared_signal_or_the_wrong_type_takes_none_of_the_push() {
    let sink = sink();
    let catalog =
        Catalog::new()
            .entity(Entity::new("e"))
            .signal(Signal::new("e", "s", ValueType::Double));
    sink.declare(&catalog).unwrap();
    let at = std::time::UNIX_EPOCH;
    let good = Reading::new("e", "s", at, Kind::DoubleValue(1.0));
    let unknown = Reading::new("e", "nope", at, Kind::DoubleValue(1.0));
    let pushed = sink.push([good.clone(), unknown]);
    assert!(matches!(pushed, Err(SinkError::Undeclared { .. })));
    let wrong = Reading::new("e", "s", at, Kind::BoolValue(true));
    let pushed = sink.push([good, wrong]);
    assert!(matches!(
        pushed,
        Err(SinkError::WrongType {
            got: ValueType::Bool,
            ..
        })
    ));
    assert_eq!(sink.health().held, 0);
}

#[tokio::test]
async fn a_sequence_given_is_kept_and_one_not_given_is_counted() {
    let socket = Socket::new("sequence");
    let log = Log::default();
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let running = started(socket.endpoint(), Options::default());
    let at = std::time::UNIX_EPOCH;
    let own = Reading::new("e", "s", at, Kind::DoubleValue(1.0)).sequence(77);
    running.sink.push([own]).unwrap();
    running.sink.push(readings(0, 1)).unwrap();
    acknowledged(&running.sink, 2).await;
    assert_eq!(log.lock().unwrap().stored, [77, 2]);
    running.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn shared_memory_asked_of_a_norn_that_grants_none_appends_over_the_socket() {
    let socket = Socket::new("shm");
    let log = Log::default();
    let daemon = Daemon::start(&socket.endpoint(), log.clone()).await;
    let told = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&told);
    let options = Options::default()
        .shared_memory(true)
        .on_notice(move |notice: &Notice| keep.lock().unwrap().push(notice.clone()));
    let sink = Sink::new(socket.endpoint(), "plant", options).unwrap();
    let catalog =
        Catalog::new()
            .entity(Entity::new("e"))
            .signal(Signal::new("e", "s", ValueType::Double));
    sink.declare(&catalog).unwrap();
    let running = run(sink);
    running.sink.push(readings(0, 3)).unwrap();
    acknowledged(&running.sink, 3).await;
    assert_eq!(log.lock().unwrap().stored, [1, 2, 3]);
    let said = if cfg!(feature = "shared-memory") {
        Notice::Plane {
            shared_memory: false,
        }
    } else {
        Notice::NoSharedMemory
    };
    assert!(told.lock().unwrap().contains(&said), "{told:?}");
    running.close().await;
    daemon.stop().await;
}
