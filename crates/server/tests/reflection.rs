//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Schema discovery on owner-only IPC, including Postman's v1alpha client.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
#[path = "shm_common/mod.rs"]
mod shm_common;

use norn_server::DaemonConfig;
use prost::Message;
use std::collections::{HashSet, VecDeque};
use tonic::Code;
use tonic::codegen::tokio_stream::iter;
use tonic_reflection::pb::{v1, v1alpha};

#[tokio::test(flavor = "multi_thread")]
async fn socket_clients_can_discover_services_and_fetch_their_complete_schema() {
    let d = shm_common::start("").await;
    let request = v1::ServerReflectionRequest {
        message_request: Some(v1::server_reflection_request::MessageRequest::ListServices(
            "".into(),
        )),
        ..Default::default()
    };
    let mut client = v1::server_reflection_client::ServerReflectionClient::new(d.channel.clone());
    let mut replies = client
        .server_reflection_info(iter([request]))
        .await
        .unwrap()
        .into_inner();
    let reply = replies.message().await.unwrap().unwrap();
    let Some(v1::server_reflection_response::MessageResponse::ListServicesResponse(list)) =
        reply.message_response
    else {
        panic!("expected the service list")
    };
    assert!(
        list.service
            .iter()
            .any(|s| s.name == "norn.admin.v1.AdminService")
    );
    assert!(
        list.service
            .iter()
            .any(|s| s.name == "norn.telemetry.v1.TelemetryService")
    );
    let request = v1alpha::ServerReflectionRequest {
        message_request: Some(
            v1alpha::server_reflection_request::MessageRequest::FileContainingSymbol(
                "norn.admin.v1.AdminService".into(),
            ),
        ),
        ..Default::default()
    };
    let mut alpha =
        v1alpha::server_reflection_client::ServerReflectionClient::new(d.channel.clone());
    let mut replies = alpha
        .server_reflection_info(iter([request]))
        .await
        .unwrap()
        .into_inner();
    let reply = replies.message().await.unwrap().unwrap();
    let Some(v1alpha::server_reflection_response::MessageResponse::FileDescriptorResponse(files)) =
        reply.message_response
    else {
        panic!("expected the service descriptors")
    };
    let descriptors: Vec<_> = files
        .file_descriptor_proto
        .iter()
        .map(|b| prost_types::FileDescriptorProto::decode(b.as_slice()).unwrap())
        .collect();
    assert!(
        descriptors
            .iter()
            .any(|f| f.name.as_deref() == Some("norn/admin/v1/admin_service.proto"))
    );
    drop(replies);
    // Clients discover imported files by name; walk every dependency as Postman does.
    let mut pending: VecDeque<_> = descriptors.into_iter().flat_map(|f| f.dependency).collect();
    let mut found = HashSet::new();
    while let Some(name) = pending.pop_front() {
        if !found.insert(name.clone()) {
            continue;
        }
        let request = v1alpha::ServerReflectionRequest {
            message_request: Some(
                v1alpha::server_reflection_request::MessageRequest::FileByFilename(name.clone()),
            ),
            ..Default::default()
        };
        let mut replies = alpha
            .server_reflection_info(iter([request]))
            .await
            .unwrap()
            .into_inner();
        let reply = replies.message().await.unwrap().unwrap();
        let Some(v1alpha::server_reflection_response::MessageResponse::FileDescriptorResponse(
            files,
        )) = reply.message_response
        else {
            panic!("missing import {name}")
        };
        for bytes in files.file_descriptor_proto {
            let file = prost_types::FileDescriptorProto::decode(bytes.as_slice()).unwrap();
            assert_eq!(file.name.as_deref(), Some(name.as_str()));
            pending.extend(file.dependency);
        }
    }
    assert!(found.contains("google/api/annotations.proto"));
    assert!(found.contains("mcp/v1/annotations.proto"));
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn loopback_tcp_does_not_expose_reflection() {
    let d = common::start(DaemonConfig::default(), common::temp("reflection-tcp")).await;
    let request = v1::ServerReflectionRequest {
        message_request: Some(v1::server_reflection_request::MessageRequest::ListServices(
            "".into(),
        )),
        ..Default::default()
    };
    let mut client = v1::server_reflection_client::ServerReflectionClient::new(d.channel.clone());
    let status = client
        .server_reflection_info(iter([request]))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    d.stop().await;
}
