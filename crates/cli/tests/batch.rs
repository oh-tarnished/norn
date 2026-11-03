//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Batch creates from a client, read back by the real nornctl binary, against
//! a real daemon: what a site's adapter does at every start.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_ipc::Endpoint;
use norn_protocol::common::ValueType;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{BatchCreateEntitiesRequest, CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{BatchCreateSignalsRequest, CreateSignalRequest, Signal};
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh Unix socket; on Windows, where tokio has none, a named pipe.
#[cfg(unix)]
fn endpoint() -> Endpoint {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nbat{}-{n}", std::process::id()));
    Endpoint::Unix(dir.join("n.sock"))
}

#[cfg(windows)]
fn endpoint() -> Endpoint {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = format!(r"pipe:\\.\pipe\norn-bat-{}-{n}", std::process::id());
    name.parse().unwrap()
}

/// nornctl's standard output, asserting it succeeded.
fn nornctl(endpoint: &Endpoint, command: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args(["--endpoint", &endpoint.to_string(), "--timeout", "5"])
        .args(command.split(' '))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// Every device and point of a small site, as one batch call each.
async fn provision(channel: &Channel) -> (Vec<Entity>, Vec<Signal>) {
    let entities = BatchCreateEntitiesRequest {
        requests: (0..4)
            .map(|d| CreateEntityRequest {
                entity_id: format!("device-{d}"),
                entity: Some(Entity::default()),
                validate_only: false,
            })
            .collect(),
        validate_only: false,
    };
    let mut client = EntityServiceClient::new(channel.clone());
    let entities = client.batch_create_entities(entities).await.unwrap();
    let requests = (0..400)
        .map(|n| CreateSignalRequest {
            parent: format!("entities/device-{}", n / 100),
            signal_id: format!("ai-{}", n % 100),
            signal: Some(Signal {
                value_type: ValueType::Double as i32,
                stream: "streams/plant".into(),
                ..Signal::default()
            }),
            validate_only: false,
        })
        .collect();
    let signals = BatchCreateSignalsRequest {
        parent: "entities/-".into(),
        requests,
        validate_only: false,
    };
    let mut client = SignalServiceClient::new(channel.clone());
    let signals = client.batch_create_signals(signals).await.unwrap();
    (entities.into_inner().entities, signals.into_inner().signals)
}

// Multi-threaded: the test blocks on nornctl while the daemon serves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_site_provisioned_in_batches_twice_lists_once_in_nornctl() {
    let wanted = endpoint();
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let data = std::env::temp_dir().join(format!("nbat-data-{}-{n}", std::process::id()));
    let config = DaemonConfig::default()
        .listen(wanted.clone())
        .data_dir(data.clone());
    let daemon = Daemon::bind(config).await.unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    assert_eq!(health.reached(Health::Serving).await, Health::Serving);

    nornctl(&wanted, "streams create plant");
    let channel = norn_ipc::connect(&wanted).await.unwrap();
    let first = provision(&channel).await;
    let again = provision(&channel).await;
    assert_eq!(first, again, "a repeated start gets the stored catalogue");

    let entities = nornctl(&wanted, "entities");
    assert_eq!(entities.lines().count(), 4, "{entities}");
    let listed = nornctl(&wanted, "signals");
    assert_eq!(listed.lines().count(), 400, "{listed}");
    assert!(listed.contains("entities/device-3/signals/ai-99 VALUE_TYPE_DOUBLE streams/plant"));
    let one = nornctl(&wanted, "signals device-2");
    assert_eq!(one.lines().count(), 100, "{one}");

    cancel.cancel();
    task.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(data);
}
