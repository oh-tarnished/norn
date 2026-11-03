//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn.telemetry.v1.TelemetryService`: `AppendSamples`.

mod batch;
mod reply;

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
use crate::metered::Held;
use crate::unimplemented::not_implemented;

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

    /// Reads batches until the client stops, or the daemon starts draining:
    /// first responses in batch order, each admitted batch's completion queued
    /// for [`forward`]. True when it stopped for the drain.
    ///
    /// A client may hold a call open between batches for as long as it likes,
    /// and a call in progress keeps its connection through HTTP/2's graceful
    /// GOAWAY. So without the drain arm an idle append held every shutdown
    /// until the drain bound cut it.
    async fn read(
        self,
        mut input: Streaming<AppendSamplesRequest>,
        held: Option<Arc<Held>>,
        out: mpsc::Sender<Result<AppendSamplesResponse, Status>>,
        waits: mpsc::Sender<(AppendSamplesResponse, Completion)>,
    ) -> bool {
        let draining = self.ingest.draining();
        let mut last_batch = None;
        loop {
            let next = tokio::select! {
                biased;
                () = draining.cancelled() => return true,
                next = input.message() => next,
            };
            let request = match next {
                Ok(Some(request)) => request,
                Ok(None) => return false,
                Err(status) => {
                    let _ = out.send(Err(status)).await;
                    return false;
                }
            };
            let oversized = held.as_ref().and_then(|h| h.take_oversized());
            let full = waits.capacity() == 0;
            let (first, completion) = self.batch(request, oversized, full, &mut last_batch);
            if out.send(Ok(first.clone())).await.is_err() {
                return false;
            }
            if let Some(completion) = completion
                && waits.send((first, completion)).await.is_err()
            {
                return false;
            }
        }
    }
}

/// Sends second responses, in batch order, until `read` is done. An admitted
/// batch dropped by a dead stream reports as failed; none vanish.
async fn forward(
    mut waits: mpsc::Receiver<(AppendSamplesResponse, Completion)>,
    out: mpsc::Sender<Result<AppendSamplesResponse, Status>>,
) {
    while let Some((first, completion)) = waits.recv().await {
        let second = reply::completed(first, completion.await);
        if out.send(Ok(second)).await.is_err() {
            return;
        }
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
        let held = request.extensions().get::<Arc<Held>>().cloned();
        let input = request.into_inner();
        let capacity = self.max_pending.saturating_mul(2).max(16);
        let (out, responses) = mpsc::channel(capacity);
        let (waits_tx, waits) = mpsc::channel(self.max_pending.max(1));
        let reader = self.clone().read(input, held, out.clone(), waits_tx);
        let last = out.clone();
        self.calls.spawn(async move {
            let (drained, ()) = tokio::join!(reader, forward(waits, out));
            // Every admitted batch has had its answer: now say why the call
            // ends, so the client reconnects rather than taking it as done.
            if drained {
                let _ = last.send(Err(crate::events::shutting_down())).await;
            }
        });
        Ok(Response::new(Responses(responses)))
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
