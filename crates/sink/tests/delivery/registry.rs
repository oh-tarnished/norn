//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A registry that creates as Norn does: `BatchCreateEntities` and
//! `BatchCreateSignals` up to 1,000 at a time and [`LIMIT`] a message, refused
//! `RESOURCE_EXHAUSTED` past it, each batch costing [`COMMIT`], and remembering
//! every signal made, so the telemetry fake can say if a reading came first. It
//! never makes one at a time: a sink that falls back is seen to.

use norn_protocol::admin::admin_service_server::AdminService;
use norn_protocol::admin::{self, Limits, NegotiateProtocolRequest, NegotiateProtocolResponse};
use norn_protocol::entity::entity_service_server::EntityService;
use norn_protocol::entity::{self, Entity};
use prost::Message as _;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tonic::{Request, Response, Status, async_trait};

/// The largest message Norn 1.0.0-rc.1 takes on a unary call.
pub const LIMIT: usize = 256 << 10;
/// What one batch costs: Norn's group commit, about 86,000 signals a second.
const COMMIT: Duration = Duration::from_millis(12);

/// The signals made, by full name.
pub type Made = Arc<Mutex<HashSet<String>>>;

/// The registry. Clones share what it made.
#[derive(Clone, Default)]
pub struct Registry {
    made: Made,
    /// Signal batches created.
    pub batches: Arc<AtomicUsize>,
    /// The largest signal batch, in resources.
    pub largest: Arc<AtomicUsize>,
    /// Batches of either kind refused for passing [`LIMIT`].
    pub past_limit: Arc<AtomicUsize>,
    /// Creates of one resource: a fallback there should be no need of.
    pub singly: Arc<AtomicUsize>,
}

impl Registry {
    pub fn made(&self) -> Made {
        Arc::clone(&self.made)
    }

    fn within(&self, size: usize) -> Result<(), Status> {
        if size > LIMIT {
            self.past_limit.fetch_add(1, Ordering::SeqCst);
            return Err(Status::resource_exhausted("past the message limit"));
        }
        Ok(())
    }
}

fn no<T>() -> Result<Response<T>, Status> {
    Err(Status::unimplemented("not in this fake"))
}

#[async_trait]
impl AdminService for Registry {
    async fn negotiate_protocol(
        &self,
        _: Request<NegotiateProtocolRequest>,
    ) -> Result<Response<NegotiateProtocolResponse>, Status> {
        Ok(Response::new(NegotiateProtocolResponse {
            limits: Some(Limits {
                max_batch_samples: 10_000,
                max_batch_bytes: 4 << 20,
                max_pending_batches: 16,
                ..Limits::default()
            }),
            ..NegotiateProtocolResponse::default()
        }))
    }
    async fn get_server(
        &self,
        _: Request<admin::GetServerRequest>,
    ) -> Result<Response<admin::Server>, Status> {
        no()
    }
    async fn create_backup(
        &self,
        _: Request<admin::CreateBackupRequest>,
    ) -> Result<Response<norn_protocol::google::longrunning::Operation>, Status> {
        no()
    }
}

#[async_trait]
impl EntityService for Registry {
    async fn create_entity(
        &self,
        _: Request<entity::CreateEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        self.singly.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(Entity::default()))
    }
    async fn batch_create_entities(
        &self,
        request: Request<entity::BatchCreateEntitiesRequest>,
    ) -> Result<Response<entity::BatchCreateEntitiesResponse>, Status> {
        self.within(request.get_ref().encoded_len())?;
        tokio::time::sleep(COMMIT).await;
        let entities = request.into_inner().requests.into_iter().map(|r| {
            let mut e = r.entity.unwrap_or_default();
            e.name = format!("entities/{}", r.entity_id);
            e
        });
        let entities = entities.collect();
        Ok(Response::new(entity::BatchCreateEntitiesResponse {
            entities,
        }))
    }
    async fn get_entity(
        &self,
        _: Request<entity::GetEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        no()
    }
    async fn list_entities(
        &self,
        _: Request<entity::ListEntitiesRequest>,
    ) -> Result<Response<entity::ListEntitiesResponse>, Status> {
        no()
    }
    async fn update_entity(
        &self,
        _: Request<entity::UpdateEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        no()
    }
    async fn delete_entity(
        &self,
        _: Request<entity::DeleteEntityRequest>,
    ) -> Result<Response<()>, Status> {
        no()
    }
}

mod signals;
