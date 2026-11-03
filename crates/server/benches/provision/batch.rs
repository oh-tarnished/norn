//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Batch calls: `BatchCreateEntities`, and `BatchCreateSignals` under the
//! wildcard parent, since one batch spans several devices.

use norn_protocol::entity::BatchCreateEntitiesRequest;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::signal::BatchCreateSignalsRequest;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use tonic::transport::Channel;

use crate::site::{entity, signal};

pub async fn entities(client: &mut EntityServiceClient<Channel>, range: std::ops::Range<usize>) {
    let want = range.len();
    let request = BatchCreateEntitiesRequest {
        requests: range.map(entity).collect(),
        validate_only: false,
    };
    let made = client.batch_create_entities(request).await.unwrap();
    assert_eq!(made.into_inner().entities.len(), want);
}

pub async fn signals(
    client: &mut SignalServiceClient<Channel>,
    range: std::ops::Range<usize>,
    point: impl Fn(usize) -> (usize, usize),
) {
    let want = range.len();
    let request = BatchCreateSignalsRequest {
        parent: "entities/-".into(),
        requests: range
            .map(|n| {
                let (d, p) = point(n);
                signal(d, p)
            })
            .collect(),
        validate_only: false,
    };
    let made = client.batch_create_signals(request).await.unwrap();
    assert_eq!(made.into_inner().signals.len(), want);
}
