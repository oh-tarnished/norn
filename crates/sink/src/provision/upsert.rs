//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One resource, created, or brought up to date if a past run made it.

use super::{Failure, Resource, created, failure, last};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity, GetEntityRequest, UpdateEntityRequest};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, GetSignalRequest, Signal, UpdateSignalRequest};
use prost_types::FieldMask;
use tonic::transport::Channel;

fn mask(paths: &[&str]) -> FieldMask {
    let paths = paths.iter().map(|p| (*p).to_owned()).collect();
    FieldMask { paths }
}

/// Creates `resource`, or, if a past run made it and it has changed since,
/// brings it up to date: a read first, since a read costs Norn no fsync.
pub(super) async fn upsert(channel: Channel, resource: Resource) -> Result<(), Failure> {
    match resource {
        Resource::Entity(entity) => {
            let id = last(&entity.name).to_owned();
            let mut client = EntityServiceClient::new(channel);
            let request = CreateEntityRequest {
                entity_id: id,
                entity: Some(Entity {
                    name: String::new(),
                    ..entity.clone()
                }),
                validate_only: false,
            };
            if !created(client.create_entity(request).await)? {
                // A read costs Norn no fsync, so a restart over thousands
                // of resources that did not change costs almost nothing.
                let name = entity.name.clone();
                let now = client.get_entity(GetEntityRequest { name }).await;
                if now.is_ok_and(|n| {
                    let n = n.into_inner();
                    n.display_name == entity.display_name && n.annotations == entity.annotations
                }) {
                    return Ok(());
                }
                let request = UpdateEntityRequest {
                    entity: Some(entity),
                    update_mask: Some(mask(&["display_name", "annotations"])),
                    validate_only: false,
                };
                client
                    .update_entity(request)
                    .await
                    .map_err(|s| failure(&s))?;
            }
        }
        Resource::Signal { parent, id, signal } => {
            let mut client = SignalServiceClient::new(channel);
            let request = CreateSignalRequest {
                parent,
                signal_id: id,
                signal: Some(Signal {
                    name: String::new(),
                    ..signal.clone()
                }),
                validate_only: false,
            };
            if !created(client.create_signal(request).await)? {
                let name = signal.name.clone();
                let now = client.get_signal(GetSignalRequest { name }).await;
                if now.is_ok_and(|n| {
                    let n = n.into_inner();
                    n.display_name == signal.display_name
                        && n.unit == signal.unit
                        && n.annotations == signal.annotations
                }) {
                    return Ok(());
                }
                let request = UpdateSignalRequest {
                    signal: Some(signal),
                    update_mask: Some(mask(&["display_name", "unit", "annotations"])),
                    validate_only: false,
                };
                client
                    .update_signal(request)
                    .await
                    .map_err(|s| failure(&s))?;
            }
        }
    }
    Ok(())
}

/// Brings `resource`, which exists, up to date: one call, no create or
/// read first, since a batch create has just said it differs.
pub(super) async fn update(channel: Channel, resource: Resource) -> Result<(), Failure> {
    match resource {
        Resource::Entity(entity) => {
            let request = UpdateEntityRequest {
                entity: Some(entity),
                update_mask: Some(mask(&["display_name", "annotations"])),
                validate_only: false,
            };
            EntityServiceClient::new(channel)
                .update_entity(request)
                .await
                .map_err(|s| failure(&s))?;
        }
        Resource::Signal { signal, .. } => {
            let request = UpdateSignalRequest {
                signal: Some(signal),
                update_mask: Some(mask(&["display_name", "unit", "annotations"])),
                validate_only: false,
            };
            SignalServiceClient::new(channel)
                .update_signal(request)
                .await
                .map_err(|s| failure(&s))?;
        }
    }
    Ok(())
}
