//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a selection's catalog from the daemon, page by page.

use std::collections::BTreeSet;

use norn_protocol::entity::GetEntityRequest;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::signal::ListSignalsRequest;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use tonic::transport::Channel;

use crate::{AdapterError, Catalog, entity_of};

/// The largest page ListSignals serves.
const PAGE: i32 = 1000;

/// The signals under `parent` matching `filter`, with their entities and streams.
pub(crate) async fn catalog(
    channel: &Channel,
    parent: &str,
    filter: &str,
    max_signals: usize,
) -> Result<Catalog, AdapterError> {
    let mut client = SignalServiceClient::new(channel.clone());
    let mut catalog = Catalog::default();
    let mut token = String::new();
    loop {
        let request = ListSignalsRequest {
            parent: parent.to_owned(),
            page_size: PAGE,
            page_token: token,
            filter: filter.to_owned(),
        };
        let page = client.list_signals(request).await;
        let page = page.map_err(AdapterError::call("SignalService.ListSignals"))?;
        let page = page.into_inner();
        catalog.signals.extend(page.signals);
        if catalog.signals.len() > max_signals {
            return Err(AdapterError::LimitExceeded {
                limit: "max_signals",
                value: max_signals as u64,
            });
        }
        if page.next_page_token.is_empty() {
            break;
        }
        token = page.next_page_token;
    }
    let names = (catalog.signals.iter()).filter_map(|s| entity_of(&s.name));
    let entities: BTreeSet<String> = names.map(str::to_owned).collect();
    let mut client = EntityServiceClient::new(channel.clone());
    for name in entities {
        let got = client.get_entity(GetEntityRequest { name }).await;
        let got = got.map_err(AdapterError::call("EntityService.GetEntity"))?;
        catalog.entities.push(got.into_inner());
    }
    let streams: Vec<String> = (catalog.stream_names().iter())
        .map(|s| (*s).to_owned())
        .collect();
    let mut client = StreamServiceClient::new(channel.clone());
    for name in streams {
        let got = client.get_stream(GetStreamRequest { name }).await;
        let got = got.map_err(AdapterError::call("StreamService.GetStream"))?;
        catalog.streams.push(got.into_inner());
    }
    Ok(catalog)
}
