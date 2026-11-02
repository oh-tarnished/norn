//! grpc.health.v1 follows the runtime's lifecycle; the version gate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::entity::GetEntityRequest;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::rpc::StatusExt;
use norn_protocol::{PROTOCOL_VERSION, VERSION_METADATA_KEY};
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio_util::sync::CancellationToken;
use tonic::{Code, Request};
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;

fn loopback() -> DaemonConfig {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let data = std::env::temp_dir().join(format!("norn-health-{}-{n}", std::process::id()));
    let listen: Endpoint = "tcp:127.0.0.1:0".parse().unwrap();
    DaemonConfig::default().listen(listen).data_dir(data)
}

#[tokio::test]
async fn a_watcher_sees_serving_then_not_serving_when_draining_begins() {
    let daemon = Daemon::bind(loopback()).await.unwrap();
    let endpoint = daemon.local_endpoint().clone();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    health.reached(Health::Serving).await;

    let channel = norn_ipc::connect(&endpoint).await.unwrap();
    let mut client = HealthClient::new(channel);
    let request = HealthCheckRequest {
        service: String::new(),
    };
    let mut stream = client.watch(request).await.unwrap().into_inner();
    let bound = Duration::from_secs(10);
    let mut until = async |wanted: ServingStatus| {
        loop {
            let next = tokio::time::timeout(bound, stream.message()).await;
            let reply = next.unwrap().unwrap().unwrap();
            if reply.status == wanted as i32 {
                return;
            }
        }
    };
    until(ServingStatus::Serving).await;
    cancel.cancel();
    until(ServingStatus::NotServing).await;

    drop(stream);
    drop(client);
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.aborted, 0);
    assert_eq!(health.get(), Health::Stopped);
}

fn versioned<T>(message: T, version: &str) -> Request<T> {
    let mut request = Request::new(message);
    let value = version.parse().unwrap();
    request.metadata_mut().insert(VERSION_METADATA_KEY, value);
    request
}

#[tokio::test]
async fn version_metadata_of_another_major_is_refused_except_by_get_server() {
    let daemon = Daemon::bind(loopback()).await.unwrap();
    let endpoint = daemon.local_endpoint().clone();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    health.reached(Health::Serving).await;
    let channel = norn_ipc::connect(&endpoint).await.unwrap();
    let mut entities = EntityServiceClient::new(channel.clone());
    let get = || GetEntityRequest {
        name: "entities/a".into(),
    };

    for bad in ["99.0", "one.zero", "1"] {
        let err = entities
            .get_entity(versioned(get(), bad))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::FailedPrecondition, "{bad}");
        let info = err.get_details_error_info().unwrap();
        assert_eq!(info.reason, "ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED");
    }
    let ours = format!("{}.7", PROTOCOL_VERSION.major);
    let passes = entities
        .get_entity(versioned(get(), &ours))
        .await
        .unwrap_err();
    assert_eq!(passes.code(), Code::NotFound);

    let describe = GetServerRequest {
        name: "server".into(),
    };
    let mut admin = AdminServiceClient::new(channel);
    admin.get_server(versioned(describe, "99.0")).await.unwrap();

    cancel.cancel();
    task.await.unwrap().unwrap();
}
