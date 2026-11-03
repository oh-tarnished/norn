//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Charging decode memory to `ipc_bytes`: docs/reference/architecture.md, "Memory budgets".
//!
//! tonic buffers a whole message and prost then expands it, so a body is read
//! one message at a time through [`Metered`], which reserves that message's cost
//! before tonic sees a byte of it. The next message replaces the reservation,
//! because a handler reads its next message only after finishing the last.

mod reader;
mod wire;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response};
use http_body::{Body as HttpBody, Frame};
use norn_runtime::{Budget, Reservation};
use tonic::Status;
use tonic::body::Body;
use tower::{Layer, Service};

use self::reader::Reader;

/// The bounds a meter enforces.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeterLimits {
    /// The largest `AppendSamples` message.
    pub(crate) max_batch_bytes: usize,
    /// The most samples one may carry.
    pub(crate) max_batch_samples: usize,
}

/// What a call holds: the reservation for its current message, and how many
/// samples an oversized batch had. Shared by the body and the response future,
/// so a unary call's bytes stay reserved until its handler has answered.
#[derive(Debug, Default)]
pub(crate) struct Held {
    pub(crate) memory: Mutex<Option<Reservation>>,
    oversized: AtomicU64,
}

impl Held {
    pub(crate) fn replace(&self, reservation: Option<Reservation>) {
        *self.memory.lock().unwrap_or_else(PoisonError::into_inner) = reservation;
    }

    pub(crate) fn set_oversized(&self, samples: u64) {
        self.oversized.store(samples, Ordering::Release);
    }

    /// The sample count of the batch just read, if its samples were stripped
    /// because there were too many. Clears it.
    pub(crate) fn take_oversized(&self) -> Option<u64> {
        Some(self.oversized.swap(0, Ordering::AcqRel)).filter(|n| *n > 0)
    }
}

/// Adds [`Meter`] around a service.
///
/// Concurrency: `Clone`; clones share one budget.
#[derive(Debug, Clone)]
pub struct MeterLayer {
    budget: Budget,
    limits: MeterLimits,
}

impl MeterLayer {
    pub(crate) fn new(budget: Budget, limits: MeterLimits) -> Self {
        Self { budget, limits }
    }
}

impl<S> Layer<S> for MeterLayer {
    type Service = Meter<S>;

    fn layer(&self, inner: S) -> Meter<S> {
        Meter {
            inner,
            budget: self.budget.clone(),
            limits: self.limits,
        }
    }
}

/// Wraps each request body so decoding is paid for before it happens. A call
/// that cannot get room within 30 s fails `RESOURCE_EXHAUSTED`; until then its
/// body stops being read, which is backpressure through HTTP/2 flow control.
///
/// Performance: one copy of each message, one allocation per message.
#[derive(Debug, Clone)]
pub struct Meter<S> {
    inner: S,
    budget: Budget,
    limits: MeterLimits,
}

impl<S> Service<Request<Body>> for Meter<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let held = Arc::new(Held::default());
        let append = request.uri().path() == APPEND_PATH;
        let (mut parts, body) = request.into_parts();
        parts.extensions.insert(Arc::clone(&held));
        let reader = Reader::new(body, self.budget.clone(), self.limits, append, &held);
        let body = Body::new(Metered {
            step: None,
            reader: Some(reader),
        });
        let response = self.inner.call(Request::from_parts(parts, body));
        Box::pin(async move {
            let response = response.await;
            drop(held);
            response
        })
    }
}

const APPEND_PATH: &str = "/norn.telemetry.v1.TelemetryService/AppendSamples";

type Step = Pin<Box<dyn Future<Output = (Reader, StepResult)> + Send>>;
type StepResult = Option<Result<Frame<Bytes>, Status>>;

/// A request body delivered one metered message at a time.
struct Metered {
    step: Option<Step>,
    reader: Option<Reader>,
}

impl HttpBody for Metered {
    type Data = Bytes;
    type Error = Status;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Status>>> {
        let this = self.get_mut();
        if this.step.is_none() {
            let Some(reader) = this.reader.take() else {
                return Poll::Ready(None);
            };
            this.step = Some(Box::pin(reader.next_frame()));
        }
        let Some(step) = this.step.as_mut() else {
            return Poll::Ready(None);
        };
        match step.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready((reader, frame)) => {
                this.step = None;
                this.reader = Some(reader);
                Poll::Ready(frame)
            }
        }
    }
}
