//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn.telemetry.v1.TelemetryService`: `AppendSamples`.

pub(crate) mod arrive;
pub(crate) mod batch;
mod call;
mod grant;
mod plane;
mod pump;
mod reply;
mod source;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use norn_core::Config;
use norn_metadata::Registry;
use norn_protocol::telemetry::telemetry_service_server::{SERVICE_NAME, TelemetryService};
use norn_protocol::telemetry::{
    AppendSamplesRequest, AppendSamplesResponse, SubscribeEventsRequest,
};
use norn_runtime::ingest::{Completion, Ingest};
use tokio::sync::mpsc;
use tokio_util::task::TaskTracker;
use tonic::codegen::tokio_stream::Stream;
use tonic::{Request, Response, Status, Streaming};

use crate::events::{Events, Subscriptions};
use crate::unimplemented::not_implemented;

pub(crate) use self::plane::Plane;

/// Responses to one call: a bounded queue the client's reading drains, so a
/// client that stops reading stalls its own call and no one else's.
pub struct Responses(mpsc::Receiver<Result<AppendSamplesResponse, Status>>);

impl Stream for Responses {
    type Item = Result<AppendSamplesResponse, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// The TelemetryService implementation: `AppendSamples`, and `SubscribeEvents`
/// through [`Subscriptions`].
///
/// Each `AppendSamples` call is one task reading batches and writing responses.
/// It ends when the client closes or breaks the call, or when the daemon starts
/// draining, once every admitted batch has been answered; a drained call then
/// ends `UNAVAILABLE` with `ERROR_REASON_SHUTTING_DOWN`. Memory per call: at most `max_pending_batches` waiting
/// completions and that many responses. Concurrency: `Clone`, shared by tonic.
#[derive(Debug, Clone)]
pub struct Telemetry {
    registry: Arc<Registry>,
    ingest: Arc<Ingest>,
    max_batch_samples: usize,
    max_pending: usize,
    events: Option<Arc<Subscriptions>>,
    /// Where each call's task runs, so shutdown can wait for it.
    calls: TaskTracker,
    /// Rings for the calls that ask; `None` keeps every call on gRPC.
    plane: Option<Arc<Plane>>,
}

impl Telemetry {
    /// A service over `registry` and `ingest`, with `config`'s limits.
    #[must_use]
    pub fn new(registry: Arc<Registry>, ingest: Arc<Ingest>, config: &Config) -> Self {
        let limits = config.limits();
        Self {
            registry,
            ingest,
            max_batch_samples: limits.max_batch_samples,
            max_pending: limits.max_pending_batches,
            events: None,
            calls: TaskTracker::new(),
            plane: None,
        }
    }

    /// Runs each call's task on `calls`, which the daemon closes and waits
    /// for before closing ingestion, so no admitted batch is left unanswered.
    #[must_use]
    pub fn tracked(mut self, calls: TaskTracker) -> Self {
        self.calls = calls;
        self
    }

    /// Serves `SubscribeEvents` from `events`; without it the method is
    /// `UNIMPLEMENTED`.
    #[must_use]
    pub fn events(mut self, events: Arc<Subscriptions>) -> Self {
        self.events = Some(events);
        self
    }

    /// Offers shared-memory rings when the daemon has a plane available.
    #[must_use]
    pub(crate) fn plane(mut self, plane: Option<Arc<Plane>>) -> Self {
        self.plane = plane;
        self
    }
}

#[tonic::async_trait]
impl TelemetryService for Telemetry {
    type AppendSamplesStream = Responses;
    type SubscribeEventsStream = Events;

    async fn append_samples(
        &self,
        request: Request<Streaming<AppendSamplesRequest>>,
    ) -> Result<Response<Responses>, Status> {
        let (service, input) = self.source(request).await;
        let mut response = Response::new(self.drive(input));
        grant::announce(&mut response, service);
        Ok(response)
    }

    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Events>, Status> {
        let Some(events) = &self.events else {
            let capability = Some("CAPABILITY_LIVE_SUBSCRIPTION");
            return Err(not_implemented(SERVICE_NAME, "SubscribeEvents", capability));
        };
        Ok(Response::new(events.subscribe(request.into_inner()).await?))
    }
}

pub(crate) type Handled = (AppendSamplesResponse, Option<Completion>);
