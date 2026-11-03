//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `BatchCreateEntities` and `BatchCreateSignals` (AIP-233): each request is
//! checked here, naming its index, and the batch is one registry call, so one
//! log write and one durable commit, all or nothing.

use norn_metadata::{EntitySpec, MAX_BATCH_CREATE, SignalSpec, check_display_name};
use norn_metadata::{MetadataError, check_resource_id};
use norn_protocol::entity::{
    BatchCreateEntitiesRequest, BatchCreateEntitiesResponse, CreateEntityRequest,
};
use norn_protocol::signal::{
    BatchCreateSignalsRequest, BatchCreateSignalsResponse, CreateSignalRequest,
};
use tonic::Status;

use super::entity::to_wire;
use super::{Catalog, annotations, names, write_mode};
use crate::convert::kind_from_wire;
use crate::status::bad_field;

/// The wildcard parent of a batch whose signals belong to several entities.
const ANY_ENTITY: &str = "entities/-";

/// Refuses a batch that is empty or larger than the registry takes.
fn bounded(n: usize) -> Result<(), Status> {
    if n == 0 || n > MAX_BATCH_CREATE {
        let text = format!("must hold 1 to {MAX_BATCH_CREATE} requests");
        return Err(bad_field("requests", &text));
    }
    Ok(())
}

/// A registry check's failure, as a field of request `at`.
fn checked(at: &str, done: Result<(), MetadataError>) -> Result<(), Status> {
    done.map_err(|e| match e {
        MetadataError::Invalid { field, reason } => bad_field(&format!("{at}.{field}"), &reason),
        other => super::metadata_status(&other),
    })
}

fn not_nested(at: &str, validate_only: bool) -> Result<(), Status> {
    if validate_only {
        let field = format!("{at}.validate_only");
        return Err(bad_field(&field, "must be unset; set the batch's instead"));
    }
    Ok(())
}

impl Catalog {
    /// One `CreateEntityRequest` of a batch, as the registry takes it.
    fn entity_item(
        &self,
        at: &str,
        r: CreateEntityRequest,
    ) -> Result<(String, EntitySpec), Status> {
        not_nested(at, r.validate_only)?;
        let field = format!("{at}.entity");
        let entity = r.entity.ok_or_else(|| bad_field(&field, "is required"))?;
        checked(at, check_resource_id("entity_id", &r.entity_id))?;
        checked(&field, check_display_name(&entity.display_name))?;
        let map = annotations(&self.registry, entity.annotations)?;
        Ok((r.entity_id, EntitySpec::new(entity.display_name, map)))
    }

    pub(super) async fn batch_entities(
        &self,
        r: BatchCreateEntitiesRequest,
    ) -> Result<BatchCreateEntitiesResponse, Status> {
        bounded(r.requests.len())?;
        let mut items = Vec::with_capacity(r.requests.len());
        for (i, item) in r.requests.into_iter().enumerate() {
            items.push(self.entity_item(&format!("requests[{i}]"), item)?);
        }
        let (mode, now) = (write_mode(r.validate_only), self.clock.now());
        let made = self.mutate(move |g| g.create_entities(items, now, mode));
        let entities = made.await?.iter().map(to_wire).collect();
        Ok(BatchCreateEntitiesResponse { entities })
    }

    /// One `CreateSignalRequest` of a batch under `parent`, as the registry
    /// takes it: (entity, signal, spec).
    fn signal_item(
        &self,
        at: &str,
        parent: &str,
        r: CreateSignalRequest,
    ) -> Result<(String, String, SignalSpec), Status> {
        not_nested(at, r.validate_only)?;
        let own = match (parent, r.parent.as_str()) {
            (ANY_ENTITY, own) | (_, own @ "") => own,
            (batch, own) if batch == own => own,
            _ => {
                let field = format!("{at}.parent");
                return Err(bad_field(&field, "must equal the batch's parent"));
            }
        };
        let own = if own.is_empty() { parent } else { own };
        let entity = names::entity(own)
            .ok()
            .filter(|e| *e != "-")
            .ok_or_else(|| bad_field(&format!("{at}.parent"), "must be entities/{entity}"))?;
        let field = format!("{at}.signal");
        let signal = r.signal.ok_or_else(|| bad_field(&field, "is required"))?;
        let kind = kind_from_wire(signal.value_type)
            .map_err(|e| bad_field(&format!("{field}.value_type"), &e.to_string()))?;
        let stream = signal.stream.strip_prefix("streams/");
        let stream = stream
            .filter(|s| !s.is_empty() && !s.contains('/'))
            .ok_or_else(|| bad_field(&format!("{field}.stream"), "must be streams/{stream}"))?;
        checked(at, check_resource_id("signal_id", &r.signal_id))?;
        checked(&field, check_display_name(&signal.display_name))?;
        let map = annotations(&self.registry, signal.annotations)?;
        let spec = SignalSpec::new(kind, stream.to_owned()).describe(
            signal.display_name,
            signal.unit,
            map,
        );
        Ok((entity.to_owned(), r.signal_id, spec))
    }

    pub(super) async fn batch_signals(
        &self,
        r: BatchCreateSignalsRequest,
    ) -> Result<BatchCreateSignalsResponse, Status> {
        if r.parent != ANY_ENTITY {
            names::entity(&r.parent)
                .map_err(|_| bad_field("parent", "must be entities/{entity}"))?;
        }
        bounded(r.requests.len())?;
        let mut items = Vec::with_capacity(r.requests.len());
        for (i, item) in r.requests.into_iter().enumerate() {
            items.push(self.signal_item(&format!("requests[{i}]"), &r.parent, item)?);
        }
        let owners: Vec<String> = items.iter().map(|(e, ..)| e.clone()).collect();
        let (mode, now) = (write_mode(r.validate_only), self.clock.now());
        let made = self
            .mutate(move |g| g.create_signals(items, now, mode))
            .await?;
        let signals = owners
            .iter()
            .zip(&made)
            .map(|(e, s)| self.signal_to_wire(e, s))
            .collect();
        Ok(BatchCreateSignalsResponse { signals })
    }
}
