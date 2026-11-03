//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn.entity.v1.EntityService`.

use norn_metadata::{EntityPatch, EntityRecord, EntitySpec};
use norn_protocol::entity::entity_service_server::EntityService;
use norn_protocol::entity::{
    BatchCreateEntitiesRequest, BatchCreateEntitiesResponse, CreateEntityRequest,
    DeleteEntityRequest, Entity, GetEntityRequest, ListEntitiesRequest, ListEntitiesResponse,
    UpdateEntityRequest,
};
use tonic::{Request, Response, Status};

use super::query::{Mask, after_token, matches, page_size, page_token, parse_filter};
use super::{
    Catalog, annotation_map, annotations, etag, expected, metadata_status, names, write_mode,
};
use crate::convert::time_to_wire;
use crate::status::bad_field;

const FILTERABLE: [&str; 1] = ["display_name"];
const MUTABLE: [&str; 2] = ["display_name", "annotations"];

pub(super) fn to_wire(r: &EntityRecord) -> Entity {
    Entity {
        name: format!("entities/{}", r.descriptor.name),
        display_name: r.display_name.clone(),
        annotations: annotation_map(&r.descriptor.metadata),
        create_time: Some(time_to_wire(r.create_time)),
        update_time: Some(time_to_wire(r.update_time)),
        etag: etag(r.revision),
    }
}

#[tonic::async_trait]
impl EntityService for Catalog {
    async fn create_entity(
        &self,
        request: Request<CreateEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        let r = request.into_inner();
        let entity = r.entity.ok_or_else(|| bad_field("entity", "is required"))?;
        let map = annotations(&self.registry, entity.annotations)?;
        let spec = EntitySpec::new(entity.display_name, map);
        let (id, mode, now) = (r.entity_id, write_mode(r.validate_only), self.clock.now());
        let made = self.mutate(move |g| g.create_entity(&id, spec, now, mode));
        Ok(Response::new(to_wire(&made.await?)))
    }

    async fn batch_create_entities(
        &self,
        request: Request<BatchCreateEntitiesRequest>,
    ) -> Result<Response<BatchCreateEntitiesResponse>, Status> {
        Ok(Response::new(
            self.batch_entities(request.into_inner()).await?,
        ))
    }

    async fn get_entity(
        &self,
        request: Request<GetEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        let name = request.into_inner().name;
        let record = self.registry.entity(names::entity(&name)?);
        let record = record.map_err(|e| metadata_status(&e))?;
        Ok(Response::new(to_wire(&record)))
    }

    async fn list_entities(
        &self,
        request: Request<ListEntitiesRequest>,
    ) -> Result<Response<ListEntitiesResponse>, Status> {
        let r = request.into_inner();
        let size = page_size(r.page_size)?;
        let clauses = parse_filter(&r.filter, &FILTERABLE)?;
        let after = after_token(&r.page_token, "", &r.filter)?;
        let mut page = self.registry.list_entities(after, size + 1, |e| {
            matches(&clauses, |field| match field {
                "display_name" => Some(e.display_name.clone()),
                other => other
                    .strip_prefix("annotations.")
                    .and_then(|k| e.descriptor.metadata.get(k))
                    .map(str::to_owned),
            })
        });
        let more = page.len() > size;
        page.truncate(size);
        let next_page_token = match page.last() {
            Some(last) if more => page_token("", &r.filter, last.descriptor.name.as_str()),
            _ => String::new(),
        };
        Ok(Response::new(ListEntitiesResponse {
            entities: page.iter().map(to_wire).collect(),
            next_page_token,
        }))
    }

    async fn update_entity(
        &self,
        request: Request<UpdateEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        let r = request.into_inner();
        let entity = r.entity.ok_or_else(|| bad_field("entity", "is required"))?;
        let id = names::entity(&entity.name)?.to_owned();
        let mask = Mask::parse(r.update_mask, &MUTABLE)?;
        let name = mask.wants("display_name", !entity.display_name.is_empty());
        let notes = mask.wants("annotations", !entity.annotations.is_empty());
        let notes = notes
            .then(|| annotations(&self.registry, entity.annotations))
            .transpose()?;
        let patch = EntityPatch::new(name.then_some(entity.display_name), notes);
        let (want, mode, now) = (
            expected(&entity.etag),
            write_mode(r.validate_only),
            self.clock.now(),
        );
        let done = self.mutate(move |g| g.update_entity(&id, patch, want, now, mode));
        Ok(Response::new(to_wire(&done.await?)))
    }

    async fn delete_entity(
        &self,
        request: Request<DeleteEntityRequest>,
    ) -> Result<Response<()>, Status> {
        let r = request.into_inner();
        let id = names::entity(&r.name)?.to_owned();
        let (want, mode) = (expected(&r.etag), write_mode(r.validate_only));
        self.mutate(move |g| g.delete_entity(&id, want, mode))
            .await?;
        Ok(Response::new(()))
    }
}
