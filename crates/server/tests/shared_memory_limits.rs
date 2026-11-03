//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The shared-memory plane's limits: refused at configuration, refused per
//! batch exactly as the socket refuses, and hidden from clients before 1.7.
// Rings are never offered on Windows (docs/reference/shared-memory.md).
#![cfg(unix)]
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[path = "common/mod.rs"]
mod common;
#[path = "shm_common/mod.rs"]
mod shm_common;

use common::{append, batch, double};
use norn_ipc::DataPlaneKind;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, NegotiateProtocolRequest, ProtocolVersion};
use norn_protocol::rpc::pb;
use norn_protocol::telemetry::AcknowledgementLevel::{Buffered, Durable};
use norn_protocol::telemetry::AppendSamplesResponse;
use norn_server::{ConfigError, DaemonConfig, SharedMemorySettings};
use shm_common::{call, finish, on, start};

fn refused_setting(toml: &str) -> &'static str {
    match DaemonConfig::from_toml(toml) {
        Err(ConfigError::Invalid { setting, .. }) => setting,
        other => panic!("expected a refusal of {toml:?}, got {other:?}"),
    }
}

#[test]
fn shared_memory_is_off_by_default_and_unchecked_while_off() {
    let config = DaemonConfig::from_toml("").unwrap();
    assert!(!config.shared_memory_settings().enabled);
    assert_eq!(
        SharedMemorySettings::default(),
        *config.shared_memory_settings()
    );
    let off = "[shared_memory]\nenabled = false\nslot_count = 0";
    assert!(DaemonConfig::from_toml(off).is_ok());
}

#[test]
fn limits_past_their_bounds_are_refused_at_startup() {
    assert_eq!(
        refused_setting(&on("slot_count = 0")),
        "shared_memory.slot_count"
    );
    assert_eq!(
        refused_setting(&on("slot_count = 1025")),
        "shared_memory.slot_count"
    );
    assert_eq!(
        refused_setting(&on("max_rings = 0")),
        "shared_memory.max_rings"
    );
    let tiny = on("max_frame_bytes = 1024");
    assert_eq!(refused_setting(&tiny), "shared_memory.max_frame_bytes");
    let past_a_batch = on("max_frame_bytes = 8388608");
    assert_eq!(
        refused_setting(&past_a_batch),
        "shared_memory.max_frame_bytes"
    );
    let name = on("service = \"a/b\"");
    assert_eq!(refused_setting(&name), "shared_memory.service");
}

#[test]
fn rings_that_do_not_fit_the_ipc_budget_are_refused_at_startup() {
    let toml = format!(
        "[memory]\nipc_bytes = 33554432\n{}",
        on("max_rings = 64\nslot_count = 64\nmax_frame_bytes = 1048576")
    );
    assert_eq!(refused_setting(&toml), "shared_memory");
}

fn reason_of(r: &AppendSamplesResponse) -> String {
    use prost::Message;
    let status = r.error.as_ref().unwrap();
    let any = status
        .details
        .iter()
        .find(|d| d.type_url.ends_with("ErrorInfo"));
    pb::ErrorInfo::decode(&any.unwrap().value[..])
        .unwrap()
        .reason
}

fn outcome(r: &AppendSamplesResponse) -> (i64, i32, i32, String) {
    let a = r.admission.as_ref().unwrap();
    (r.batch_id, a.outcome, a.reason, reason_of(r))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_past_the_sample_limit_is_refused_through_a_ring_as_on_the_socket() {
    let toml = format!("[limits]\nmax_batch_samples = 10\n{}", on(""));
    let d = start(&toml).await;
    let too_many = || batch(7, (0..11).map(|n| double("t", n)).collect(), Durable);
    let socket = append(&d.channel, vec![too_many()]).await;
    let call = call(&d.channel).await;
    assert_eq!(call.plane(), DataPlaneKind::SharedMemory);
    call.send(too_many()).await.unwrap();
    call.send(batch(8, vec![double("t", 100)], Buffered))
        .await
        .unwrap();
    let ring = finish(call).await;
    assert_eq!(outcome(&ring[0]), outcome(&socket[0]), "the same refusal");
    assert_eq!(reason_of(&ring[0]), "ERROR_REASON_LIMIT_EXCEEDED");
    assert_eq!(ring[1].batch_id, 8, "the call survives the refusal");
    assert!(ring[1].error.is_none());
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_before_1_7_sees_the_capability_but_not_the_plane() {
    let d = start(&on("")).await;
    let request = NegotiateProtocolRequest {
        name: "server".into(),
        client_version: Some(ProtocolVersion { major: 1, minor: 6 }),
        required_capabilities: vec![Capability::SharedMemory as i32],
        user_agent: "old".into(),
    };
    let reply = AdminServiceClient::new(d.channel.clone())
        .negotiate_protocol(request)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reply.protocol_version.unwrap().minor, 6);
    assert!(reply.shared_memory.is_none());
    let current = shm_common::negotiated(&d.channel).await;
    let plane = current.shared_memory.unwrap();
    assert_eq!((plane.slot_count, plane.max_rings), (4, 2));
    assert_eq!(plane.segment_bytes, 6 * plane.max_frame_bytes);
    assert!(plane.service.starts_with("norn/"));
    d.stop().await;
}
