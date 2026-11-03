//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Requests for the batch-create tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use norn_protocol::common::ValueType;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::rpc::StatusExt;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{
    BatchCreateSignalsRequest, CreateSignalRequest, ListSignalsRequest, Signal,
};

pub fn entity(id: &str) -> CreateEntityRequest {
    CreateEntityRequest {
        entity_id: id.into(),
        entity: Some(Entity::default()),
        validate_only: false,
    }
}

pub fn signal(entity: &str, id: &str, kind: ValueType) -> CreateSignalRequest {
    CreateSignalRequest {
        parent: format!("entities/{entity}"),
        signal_id: id.into(),
        signal: Some(Signal {
            value_type: kind as i32,
            stream: "streams/plant".into(),
            unit: "degree_celsius".into(),
            ..Signal::default()
        }),
        validate_only: false,
    }
}

pub fn signals(parent: &str, requests: Vec<CreateSignalRequest>) -> BatchCreateSignalsRequest {
    BatchCreateSignalsRequest {
        parent: parent.into(),
        requests,
        validate_only: false,
    }
}

pub fn field(status: &tonic::Status) -> String {
    let info = status.get_details_error_info().expect("an ErrorInfo");
    info.metadata.get("field").cloned().unwrap_or_default()
}

pub async fn count(client: &mut SignalServiceClient<tonic::transport::Channel>) -> usize {
    let list = ListSignalsRequest {
        parent: "entities/-".into(),
        page_size: 1000,
        ..Default::default()
    };
    client
        .list_signals(list)
        .await
        .unwrap()
        .into_inner()
        .signals
        .len()
}
