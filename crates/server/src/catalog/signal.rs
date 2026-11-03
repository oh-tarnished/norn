//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn.signal.v1.SignalService`.

use norn_metadata::{SignalPatch, SignalRecord, SignalSpec};
use norn_protocol::signal::signal_service_server::SignalService;
use norn_protocol::signal::{
    BatchCreateSignalsRequest, BatchCreateSignalsResponse, CreateSignalRequest,
    DeleteSignalRequest, GetSignalRequest, ListSignalsRequest, ListSignalsResponse, Signal,
    UpdateSignalRequest,
};
use tonic::{Request, Response, Status};

use super::query::{Mask, after_token, page_size, page_token, parse_filter};
use super::select::{FILTERABLE, signal_matches};
use super::{
    Catalog, annotation_map, annotations, etag, expected, metadata_status, names, write_mode,
};
use crate::convert::{kind_from_wire, kind_to_wire, time_to_wire};
use crate::status::bad_field;

const MUTABLE: [&str; 3] = ["display_name", "unit", "annotations"];

impl Catalog {
    pub(super) fn signal_to_wire(&self, entity: &str, r: &SignalRecord) -> Signal {
        let stream = self.registry.stream_name(r.stream).unwrap_or_default();
        Signal {
            name: format!("entities/{entity}/signals/{}", r.descriptor.name),
            display_name: r.display_name.clone(),
            value_type: kind_to_wire(r.descriptor.kind),
            stream: format!("streams/{stream}"),
            unit: r.unit.clone(),
            annotations: annotation_map(&r.descriptor.metadata),
            create_time: Some(time_to_wire(r.create_time)),
            update_time: Some(time_to_wire(r.update_time)),
            etag: etag(r.revision),
        }
    }
}

/// The signal after which a token resumes.
fn after_pair(after: Option<&str>) -> Result<Option<(String, String)>, Status> {
    after
        .map(|a| names::signal(a).map(|(e, s)| (e.to_owned(), s.to_owned())))
        .transpose()
}

#[tonic::async_trait]
impl SignalService for Catalog {
    async fn create_signal(
        &self,
        request: Request<CreateSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        let r = request.into_inner();
        let entity = names::entity(&r.parent)?.to_owned();
        let signal = r.signal.ok_or_else(|| bad_field("signal", "is required"))?;
        let kind = kind_from_wire(signal.value_type)
            .map_err(|e| bad_field("signal.value_type", &e.to_string()))?;
        let stream = names::stream(&signal.stream)?.to_owned();
        let map = annotations(&self.registry, signal.annotations)?;
        let spec = SignalSpec::new(kind, stream).describe(signal.display_name, signal.unit, map);
        let (id, mode, now) = (r.signal_id, write_mode(r.validate_only), self.clock.now());
        let owner = entity.clone();
        let made = self.mutate(move |g| g.create_signal(&owner, &id, spec, now, mode));
        Ok(Response::new(self.signal_to_wire(&entity, &made.await?)))
    }

    async fn batch_create_signals(
        &self,
        request: Request<BatchCreateSignalsRequest>,
    ) -> Result<Response<BatchCreateSignalsResponse>, Status> {
        Ok(Response::new(
            self.batch_signals(request.into_inner()).await?,
        ))
    }

    async fn get_signal(
        &self,
        request: Request<GetSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        let name = request.into_inner().name;
        let (entity, id) = names::signal(&name)?;
        let record = self.registry.signal(entity, id);
        let record = record.map_err(|e| metadata_status(&e))?;
        Ok(Response::new(self.signal_to_wire(entity, &record)))
    }

    async fn list_signals(
        &self,
        request: Request<ListSignalsRequest>,
    ) -> Result<Response<ListSignalsResponse>, Status> {
        let r = request.into_inner();
        let size = page_size(r.page_size)?;
        let clauses = parse_filter(&r.filter, &FILTERABLE)?;
        let parent = (r.parent != "entities/-")
            .then(|| names::entity(&r.parent))
            .transpose()?;
        let after = after_token(&r.page_token, &r.parent, &r.filter)?;
        let mut page = self
            .registry
            .list_signals(parent, after_pair(after)?, size + 1, |s| {
                signal_matches(&self.registry, &clauses, s)
            });
        let more = page.len() > size;
        page.truncate(size);
        let next_page_token = match page.last() {
            Some((e, s)) if more => {
                let name = format!("entities/{e}/signals/{}", s.descriptor.name);
                page_token(&r.parent, &r.filter, &name)
            }
            _ => String::new(),
        };
        let signals = page
            .iter()
            .map(|(e, s)| self.signal_to_wire(e, s))
            .collect();
        Ok(Response::new(ListSignalsResponse {
            signals,
            next_page_token,
        }))
    }

    async fn update_signal(
        &self,
        request: Request<UpdateSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        let r = request.into_inner();
        let signal = r.signal.ok_or_else(|| bad_field("signal", "is required"))?;
        let (entity, id) = names::signal(&signal.name)?;
        let (entity, id) = (entity.to_owned(), id.to_owned());
        let mask = Mask::parse(r.update_mask, &MUTABLE)?;
        let name = mask.wants("display_name", !signal.display_name.is_empty());
        let unit = mask.wants("unit", !signal.unit.is_empty());
        let notes = mask.wants("annotations", !signal.annotations.is_empty());
        let notes = notes
            .then(|| annotations(&self.registry, signal.annotations))
            .transpose()?;
        let patch = SignalPatch::new(
            name.then_some(signal.display_name),
            unit.then_some(signal.unit),
            notes,
        );
        let (want, mode, now) = (
            expected(&signal.etag),
            write_mode(r.validate_only),
            self.clock.now(),
        );
        let owner = entity.clone();
        let done = self.mutate(move |g| g.update_signal(&owner, &id, patch, want, now, mode));
        Ok(Response::new(self.signal_to_wire(&entity, &done.await?)))
    }

    async fn delete_signal(
        &self,
        request: Request<DeleteSignalRequest>,
    ) -> Result<Response<()>, Status> {
        let r = request.into_inner();
        let (entity, id) = names::signal(&r.name)?;
        let (entity, id) = (entity.to_owned(), id.to_owned());
        let (want, mode) = (expected(&r.etag), write_mode(r.validate_only));
        self.mutate(move |g| g.delete_signal(&entity, &id, want, mode))
            .await?;
        Ok(Response::new(()))
    }
}
