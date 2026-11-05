//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reflecting the runtime's health into `grpc.health.v1`.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use norn_protocol::admin::admin_service_server::SERVICE_NAME as ADMIN;
use norn_runtime::{Health, HealthWatch, TaskError};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};
use tonic::codegen::tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tonic_health::ServingStatus;
use tonic_health::pb::health_check_response::ServingStatus as PbStatus;
use tonic_health::pb::health_server::{Health as HealthApi, HealthServer};
use tonic_health::pb::{HealthCheckRequest, HealthCheckResponse};
use tonic_health::server::{HealthReporter, HealthService};

/// The health service names the daemon reports: `""` (the whole daemon,
/// what `nornctl status` and most probes ask about) and AdminService.
const SERVICES: [&str; 2] = ["", ADMIN];

/// The gRPC status for a runtime state: `SERVING` only while serving, so a
/// load balancer or supervisor stops sending work the moment draining
/// begins.
#[must_use]
pub fn serving_status(health: Health) -> ServingStatus {
    if health.is_serving() {
        ServingStatus::Serving
    } else {
        ServingStatus::NotServing
    }
}

/// Sets every reported service to `status`.
pub(crate) async fn report(reporter: &HealthReporter, status: ServingStatus) {
    for name in SERVICES {
        reporter.set_service_status(name, status).await;
    }
}

/// A runtime task: mirrors `watch` into `reporter` until the runtime stops
/// or `token` is cancelled, then reports `NOT_SERVING` and returns. Holds
/// nothing but the two handles, so it stops within one poll of either.
pub(crate) async fn reflect(
    mut watch: HealthWatch,
    reporter: HealthReporter,
    token: CancellationToken,
) -> Result<(), TaskError> {
    let mut current = watch.get();
    loop {
        report(&reporter, serving_status(current)).await;
        if current == Health::Stopped {
            return Ok(());
        }
        tokio::select! {
            () = token.cancelled() => {
                report(&reporter, ServingStatus::NotServing).await;
                return Ok(());
            }
            next = watch.changed() => current = next,
        }
    }
}

/// The health service and the reporter that sets its statuses, its watch
/// streams ending when `draining` is cancelled (see [`Draining`]).
pub(crate) fn health_server(
    draining: CancellationToken,
) -> (HealthReporter, HealthServer<Draining>) {
    let reporter = HealthReporter::new();
    let service = HealthService::from_health_reporter(reporter.clone());
    (
        reporter,
        HealthServer::new(Draining::new(service, draining)),
    )
}

/// `grpc.health.v1.Health` whose `Watch` streams end with the drain.
///
/// tonic-health's `Watch` never ends on its own, and a call in progress keeps its
/// connection through HTTP/2's graceful GOAWAY, so one held watcher kept every
/// shutdown waiting for the drain bound. Here each watch stream, once `draining`
/// is cancelled, delivers what is already queued, then `NOT_SERVING` if that was
/// not the last status sent (the reporter may not have caught up yet), and ends `UNAVAILABLE` with `ERROR_REASON_SHUTTING_DOWN`, as an
/// idle `AppendSamples` call does. `Check` is the inner service's.
///
/// Concurrency: `Send + Sync`, shared by tonic; each stream owns a clone of the
/// token. Memory: none beyond the inner service's.
#[derive(Debug)]
pub(crate) struct Draining {
    inner: HealthService,
    draining: CancellationToken,
}

impl Draining {
    /// `inner`, its watch streams ended when `draining` is cancelled.
    pub(crate) fn new(inner: HealthService, draining: CancellationToken) -> Self {
        Self { inner, draining }
    }
}

#[tonic::async_trait]
impl HealthApi for Draining {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        self.inner.check(request).await
    }

    type WatchStream = Watch;

    async fn watch(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let inner = self.inner.watch(request).await?.into_inner();
        let ended = Box::pin(self.draining.clone().cancelled_owned());
        Ok(Response::new(Watch {
            inner,
            ended: Some(ended),
            last: None,
        }))
    }
}

/// One watch stream that ends with the drain; see [`Draining`].
pub(crate) struct Watch {
    inner: tonic_health::server::WatchStream,
    /// `None` once the end has been reported.
    ended: Option<Pin<Box<WaitForCancellationFutureOwned>>>,
    /// The status last sent, so the end follows a `NOT_SERVING` exactly once.
    last: Option<i32>,
}

impl Stream for Watch {
    type Item = Result<HealthCheckResponse, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Some(ended) = self.ended.as_mut() else {
            return Poll::Ready(None);
        };
        let over = ended.as_mut().poll(cx).is_ready();
        // Whatever is ready goes first, so the end follows the drain's NOT_SERVING.
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(item)) => {
                self.last = item.as_ref().ok().map(|r| r.status);
                return Poll::Ready(Some(item));
            }
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending if !over => return Poll::Pending,
            Poll::Pending => {}
        }
        // The drain began before the reporter said so: say it, then end.
        let not_serving = PbStatus::NotServing as i32;
        if self.last != Some(not_serving) {
            self.last = Some(not_serving);
            let status = not_serving;
            return Poll::Ready(Some(Ok(HealthCheckResponse { status })));
        }
        self.ended = None;
        Poll::Ready(Some(Err(crate::events::shutting_down())))
    }
}
