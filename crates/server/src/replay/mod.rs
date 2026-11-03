//! `norn.replay.v1.ReplayService`: sessions and their clocks.

mod drive;
mod list;
pub(crate) mod request;
pub(crate) mod wire;

use std::sync::Arc;

use norn_metadata::Registry;
use norn_protocol::replay::replay_service_server::ReplayService;
use norn_protocol::replay::{
    CreateReplaySessionRequest, DeleteReplaySessionRequest, DriveReplaySessionRequest,
    DriveReplaySessionResponse, GetReplaySessionRequest, ListReplaySessionsRequest,
    ListReplaySessionsResponse, PauseReplaySessionRequest, ReplaySession,
    ResumeReplaySessionRequest, SeekReplaySessionRequest, StepReplaySessionRequest,
    StopReplaySessionRequest, UpdateReplaySessionRequest,
};
use norn_runtime::query::Engine;
use norn_runtime::replay::Control;
use norn_runtime::sessions::Replays;
use tonic::{Request, Response, Status};

use crate::status::bad_field;
use wire::{generation, id_of, session, status, valid_id};

/// The ReplayService implementation, over the runtime's [`Replays`].
///
/// Concurrency: `Clone`, shared by tonic. A control waits for the session's task,
/// which answers between batches; a step answers once its events reached every
/// subscriber. Memory: none of its own. Failure: every refusal is a typed status,
/// as docs/reference/replay.md lists.
#[derive(Debug, Clone)]
pub struct ReplaySessions {
    registry: Arc<Registry>,
    replays: Arc<Replays>,
    queries: Arc<Engine>,
}

impl ReplaySessions {
    /// A service creating sessions over `registry`'s signals in `replays`, counting
    /// what a drive would emit through `queries` before it moves anything.
    #[must_use]
    pub fn new(registry: Arc<Registry>, replays: Arc<Replays>, queries: Arc<Engine>) -> Self {
        Self {
            registry,
            replays,
            queries,
        }
    }

    async fn control(
        &self,
        name: &str,
        etag: &str,
        control: Control,
    ) -> Result<Response<ReplaySession>, Status> {
        let id = id_of(name, "name")?;
        let info = self.replays.control(id, generation(etag)?, control).await;
        Ok(Response::new(session(&info.map_err(|e| status(&e, id))?)))
    }
}

#[tonic::async_trait]
impl ReplayService for ReplaySessions {
    async fn create_replay_session(
        &self,
        request: Request<CreateReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        let wanted = Some(r.replay_session_id).filter(|id| !id.is_empty());
        if wanted.as_deref().is_some_and(|id| !valid_id(id)) {
            let why = "must be lower-case letters, digits and hyphens, a letter first, at most 63";
            return Err(bad_field("replay_session_id", why));
        }
        let s = r
            .replay_session
            .ok_or_else(|| bad_field("replay_session", "is required"))?;
        let registry = Arc::clone(&self.registry);
        let labels = (s.scope.clone(), s.filter.clone());
        let spec = tokio::task::spawn_blocking(move || request::spec(&registry, &s))
            .await
            .map_err(|_| Status::internal("resolving signals panicked"))??;
        let shown = wanted.clone().unwrap_or_default();
        let made = self.replays.create(wanted, spec, labels);
        Ok(Response::new(session(
            &made.map_err(|e| status(&e, &shown))?,
        )))
    }

    async fn get_replay_session(
        &self,
        request: Request<GetReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let name = request.into_inner().name;
        let id = id_of(&name, "name")?;
        let info = self.replays.get(id).map_err(|e| status(&e, id))?;
        Ok(Response::new(session(&info)))
    }

    async fn list_replay_sessions(
        &self,
        request: Request<ListReplaySessionsRequest>,
    ) -> Result<Response<ListReplaySessionsResponse>, Status> {
        Ok(Response::new(list::page(
            &self.replays.sessions(),
            &request.into_inner(),
        )?))
    }

    async fn update_replay_session(
        &self,
        request: Request<UpdateReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        let s = r
            .replay_session
            .ok_or_else(|| bad_field("replay_session", "is required"))?;
        let pace = request::new_pace(&s, r.update_mask)?;
        let field = "replay_session.name";
        let id = id_of(&s.name, field)?;
        let info = self
            .replays
            .control(id, generation(&s.etag)?, Control::SetPace(pace))
            .await;
        Ok(Response::new(session(&info.map_err(|e| status(&e, id))?)))
    }

    async fn delete_replay_session(
        &self,
        request: Request<DeleteReplaySessionRequest>,
    ) -> Result<Response<()>, Status> {
        let r = request.into_inner();
        let id = id_of(&r.name, "name")?;
        let deleted = self.replays.delete(id, generation(&r.etag)?).await;
        deleted.map_err(|e| status(&e, id))?;
        Ok(Response::new(()))
    }

    async fn pause_replay_session(
        &self,
        request: Request<PauseReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        self.control(&r.name, &r.etag, Control::Pause).await
    }

    async fn resume_replay_session(
        &self,
        request: Request<ResumeReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        self.control(&r.name, &r.etag, Control::Resume).await
    }

    async fn seek_replay_session(
        &self,
        request: Request<SeekReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        let to = request::time("seek_time", r.seek_time)?;
        let seek = Control::Seek {
            to,
            report_state: true,
        };
        self.control(&r.name, &r.etag, seek).await
    }

    async fn step_replay_session(
        &self,
        request: Request<StepReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        let step = request::step(&r)?;
        self.control(&r.name, &r.etag, step).await
    }

    async fn stop_replay_session(
        &self,
        request: Request<StopReplaySessionRequest>,
    ) -> Result<Response<ReplaySession>, Status> {
        let r = request.into_inner();
        self.control(&r.name, &r.etag, Control::Stop).await
    }

    async fn drive_replay_session(
        &self,
        request: Request<DriveReplaySessionRequest>,
    ) -> Result<Response<DriveReplaySessionResponse>, Status> {
        let shared = (&self.registry, &self.queries);
        let r = request.into_inner();
        Ok(Response::new(drive::drive(&self.replays, shared, r).await?))
    }
}
