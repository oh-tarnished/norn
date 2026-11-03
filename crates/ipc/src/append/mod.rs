//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `AppendSamples` for a client that does not care which plane carries it.

mod offer;
mod replies;
mod sender;

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AppendSamplesRequest, AppendSamplesResponse};
use norn_protocol::{
    DATA_PLANE_METADATA_KEY, DATA_PLANE_SHARED_MEMORY, SHARED_MEMORY_SERVICE_METADATA_KEY,
};
use tokio::sync::mpsc;
use tonic::Status;
use tonic::codegen::tokio_stream::Stream;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

pub use self::replies::AppendReplies;
pub use self::sender::AppendSender;
use crate::{Attach, DataPlane, DataPlaneKind, SharedMemoryTicket};

/// The requests queued for gRPC when [`AppendOptions::capacity`] is not set.
pub const DEFAULT_CAPACITY: usize = 16;

/// How an [`AppendCall`] is opened. `Default` is gRPC only, with
/// [`DEFAULT_CAPACITY`]: shared memory is opt-in on the client too.
///
/// Concurrency: `Clone + Send + Sync`; clones share one [`Attach`].
#[derive(Clone)]
pub struct AppendOptions {
    capacity: usize,
    attach: Option<Arc<dyn Attach>>,
}

impl Default for AppendOptions {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            attach: None,
        }
    }
}

impl fmt::Debug for AppendOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppendOptions")
            .field("capacity", &self.capacity)
            .field("shared_memory", &self.attach.is_some())
            .finish()
    }
}

impl AppendOptions {
    /// Bounds the requests queued for gRPC; at least 1.
    #[must_use]
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity.max(1);
        self
    }

    /// Asks the daemon for a shared-memory ring, attached through `attach`
    /// (`norn_shared_memory::SharedMemory`). Without it every call is gRPC only.
    #[must_use]
    pub fn shared_memory(mut self, attach: Arc<dyn Attach>) -> Self {
        self.attach = Some(attach);
        self
    }
}

struct Requests(mpsc::Receiver<AppendSamplesRequest>);

impl Stream for Requests {
    type Item = AppendSamplesRequest;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// One `AppendSamples` call. With [`AppendOptions::shared_memory`] it negotiates
/// protocol 1.7, asks for a ring and uses one if the daemon grants it; a daemon
/// that is older, has shared memory off, is out of rings, or cannot be reached
/// through one leaves the call on gRPC, transparently.
///
/// Every response arrives over gRPC, so docs/reference/protocol.md applies
/// unchanged: a batch is admitted, or `DURABLE`, only when [`recv`](Self::recv)
/// says so. A batch larger than a ring frame goes by gRPC, and the call then
/// stays on gRPC: the daemon holds a gRPC batch until the ring is empty, but
/// cannot know one is on its way, so a later frame could overtake it.
///
/// Concurrency: one reader at a time; [`into_parts`](Self::into_parts) splits
/// sending from reading for two tasks.
#[derive(Debug)]
pub struct AppendCall {
    sender: AppendSender,
    replies: AppendReplies,
}

impl AppendCall {
    /// Opens the call over `channel`.
    ///
    /// # Errors
    ///
    /// The status the call itself fails with. Failing to reach a ring is not
    /// an error: the call is then on gRPC.
    pub async fn open(channel: Channel, options: &AppendOptions) -> Result<Self, Status> {
        let plane = match &options.attach {
            Some(_) => offer::shared_memory(channel.clone()).await,
            None => None,
        };
        let (grpc, rx) = mpsc::channel(options.capacity.max(1));
        let mut request = tonic::Request::new(Requests(rx));
        if plane.is_some() {
            let asked = MetadataValue::from_static(DATA_PLANE_SHARED_MEMORY);
            request
                .metadata_mut()
                .insert(DATA_PLANE_METADATA_KEY, asked);
        }
        let response = TelemetryServiceClient::new(channel)
            .append_samples(request)
            .await?;
        let granted = response
            .metadata()
            .get(SHARED_MEMORY_SERVICE_METADATA_KEY)
            .and_then(|value| value.to_str().ok());
        let ring: Option<Arc<dyn DataPlane>> = plane
            .zip(granted)
            .and_then(|(plane, granted)| SharedMemoryTicket::granted(&plane, granted))
            .zip(options.attach.as_ref())
            .and_then(|(ticket, attach)| attach.attach(&ticket).ok())
            .map(Arc::from);
        let sender = AppendSender::new(grpc, ring.clone());
        let replies = AppendReplies::new(response.into_inner(), ring);
        Ok(Self { sender, replies })
    }

    /// Which plane carries this call's batches now. For metrics and tests;
    /// nothing else should depend on it.
    #[must_use]
    pub fn plane(&self) -> DataPlaneKind {
        self.sender.plane()
    }

    /// Sends one batch: see [`AppendSender::send`].
    ///
    /// # Errors
    ///
    /// See [`AppendSender::send`].
    pub async fn send(&self, batch: AppendSamplesRequest) -> std::io::Result<()> {
        self.sender.send(batch).await
    }

    /// The next response, or `None` once the daemon has answered everything.
    ///
    /// # Errors
    ///
    /// The call's status.
    pub async fn recv(&mut self) -> Result<Option<AppendSamplesResponse>, Status> {
        self.replies.message().await
    }

    /// Splits off the sending half; dropping it closes the request side, after
    /// which the daemon answers what it holds and ends the call. The replies
    /// keep the publisher alive until drained, so queued frames remain readable.
    #[must_use]
    pub fn into_parts(self) -> (AppendSender, AppendReplies) {
        (self.sender, self.replies)
    }
}
