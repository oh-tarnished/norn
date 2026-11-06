//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Many resources in one call: AIP-233 `BatchCreateEntities` and
//! `BatchCreateSignals` (protocol 1.6), one registry group commit per batch,
//! where one create per resource waited on a commit each. A batch is all or
//! nothing, so an item Norn refuses is set aside by the index its error names
//! (`field` `requests[17].signal_id`) and the rest sent again. A resource a past
//! run made comes back as stored; one that has since changed is handed back to
//! be brought up to date later, one at a time, by `upsert`.

use super::{Failure, Resource, failure, last};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{BatchCreateEntitiesRequest, CreateEntityRequest, Entity};
use norn_protocol::rpc::StatusExt;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{BatchCreateSignalsRequest, CreateSignalRequest, Signal};
use std::collections::HashMap;
use tonic::transport::Channel;
use tonic::{Code, Status};

mod stale;
#[cfg(test)]
mod tests;

use stale::{Stored, stale};

/// Norn's bound on one batch (`MAX_BATCH_CREATE`).
pub(crate) const BATCH: usize = 1_000;

/// The parent of a batch whose signals belong to several entities.
const ANY_ENTITY: &str = "entities/-";

/// What a batch left to do.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Refusals, one per resource given up.
    pub refused: Vec<String>,
    /// Resources to create one at a time: all of them if this daemon has no
    /// batch create, or a refusal named none of them.
    pub each: Vec<Resource>,
    /// Resources that exist but have changed since a past run: to update.
    pub stale: Vec<Resource>,
}

/// Creates `taken`, all entities or all signals, in as few batches as Norn's
/// refusals allow: a batch past the message limit is sent again in halves,
/// never one at a time. `Err` only when the daemon went away.
pub(crate) async fn create(channel: &Channel, taken: Vec<Resource>) -> Result<Outcome, Failure> {
    let mut out = Outcome::default();
    let mut todo = vec![taken];
    while let Some(mut part) = todo.pop() {
        if part.is_empty() {
            continue;
        }
        let stored = match send(channel, &part).await {
            Ok(stored) => stored,
            Err(s) if s.code() == Code::Unimplemented => {
                // A daemon before protocol 1.6.
                out.each.extend(part);
                continue;
            }
            Err(s) if s.code() == Code::ResourceExhausted && part.len() > 1 => {
                // Past the daemon's message limit: two halves, in order.
                let second = part.split_off(part.len() / 2);
                todo.push(second);
                todo.push(part);
                continue;
            }
            Err(s) => match (failure(&s), index(&s)) {
                (Failure::Lost(why), _) => return Err(Failure::Lost(why)),
                (Failure::Refused(why), Some(i)) if i < part.len() => {
                    part.remove(i);
                    out.refused.push(why);
                    todo.push(part);
                    continue;
                }
                // A refusal naming no item: each is tried alone, so only the
                // bad ones are given up.
                (Failure::Refused(_), _) => {
                    out.each.extend(part);
                    continue;
                }
            },
        };
        out.stale
            .extend(part.into_iter().filter(|r| stale(r, &stored)));
    }
    Ok(out)
}

async fn send(channel: &Channel, taken: &[Resource]) -> Result<Stored, Status> {
    if let Some(Resource::Entity(_)) = taken.first() {
        let requests = taken
            .iter()
            .filter_map(|r| match r {
                Resource::Entity(e) => Some(CreateEntityRequest {
                    entity_id: last(&e.name).to_owned(),
                    entity: Some(Entity {
                        name: String::new(),
                        ..e.clone()
                    }),
                    validate_only: false,
                }),
                Resource::Signal { .. } => None,
            })
            .collect();
        let request = BatchCreateEntitiesRequest {
            requests,
            validate_only: false,
        };
        let mut client = EntityServiceClient::new(channel.clone());
        let held = client.batch_create_entities(request).await?.into_inner();
        let by_name = held.entities.into_iter().map(|e| (e.name.clone(), e));
        return Ok(Stored::Entities(by_name.collect::<HashMap<_, _>>()));
    }
    let mut parents = taken.iter().filter_map(|r| match r {
        Resource::Signal { parent, .. } => Some(parent.as_str()),
        Resource::Entity(_) => None,
    });
    let first = parents.next().unwrap_or(ANY_ENTITY);
    let parent = if parents.all(|p| p == first) {
        first
    } else {
        ANY_ENTITY
    };
    let requests = taken
        .iter()
        .filter_map(|r| match r {
            Resource::Signal { parent, id, signal } => Some(CreateSignalRequest {
                parent: parent.clone(),
                signal_id: id.clone(),
                signal: Some(Signal {
                    name: String::new(),
                    ..signal.clone()
                }),
                validate_only: false,
            }),
            Resource::Entity(_) => None,
        })
        .collect();
    let request = BatchCreateSignalsRequest {
        parent: parent.to_owned(),
        requests,
        validate_only: false,
    };
    let mut client = SignalServiceClient::new(channel.clone());
    let held = client.batch_create_signals(request).await?.into_inner();
    let by_name = held.signals.into_iter().map(|s| (s.name.clone(), s));
    Ok(Stored::Signals(by_name.collect()))
}

/// The item a batch error names: the `N` of `requests[N]` in its `ErrorInfo`'s
/// `field`, or its first `BadRequest` violation, as the daemon reports a
/// malformed field (protocol.md, "Malformed arguments"). Never the message.
pub(crate) fn index(status: &Status) -> Option<usize> {
    let details = status.get_error_details();
    let from_info = details
        .error_info()
        .and_then(|info| info.metadata.get("field").cloned());
    let from_bad = || {
        let bad = details.bad_request()?;
        bad.field_violations.first().map(|v| v.field.clone())
    };
    let field = from_info.or_else(from_bad)?;
    let (_, rest) = field.split_once("requests[")?;
    let (digits, _) = rest.split_once(']')?;
    digits.parse().ok()
}
