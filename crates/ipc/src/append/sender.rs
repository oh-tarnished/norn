//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The sending half of an append call: a ring when granted, gRPC otherwise.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use norn_protocol::telemetry::AppendSamplesRequest;
use prost::Message;
use tokio::sync::mpsc;

use crate::{DataPlane, DataPlaneKind};

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the append call has ended")
}

/// The sending half of an [`AppendCall`](crate::AppendCall).
///
/// Concurrency: `Send + Sync`; concurrent sends are serialised by the plane,
/// but their order is then theirs, so send from one task when order matters.
pub struct AppendSender {
    grpc: mpsc::Sender<AppendSamplesRequest>,
    ring: Option<Arc<dyn DataPlane>>,
    left_ring: AtomicBool,
}

impl std::fmt::Debug for AppendSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppendSender")
            .field("plane", &self.plane())
            .finish_non_exhaustive()
    }
}

impl AppendSender {
    pub(super) fn new(
        grpc: mpsc::Sender<AppendSamplesRequest>,
        ring: Option<Arc<dyn DataPlane>>,
    ) -> Self {
        Self {
            grpc,
            ring,
            left_ring: AtomicBool::new(false),
        }
    }

    /// Which plane carries batches now.
    #[must_use]
    pub fn plane(&self) -> DataPlaneKind {
        match &self.ring {
            Some(ring) if !self.left_ring.load(Ordering::Acquire) => ring.kind(),
            _ => DataPlaneKind::ControlPlane,
        }
    }

    /// Sends one batch, waiting while the ring or the gRPC queue is full: that
    /// is backpressure, and nothing is dropped. `Ok` says the batch was handed
    /// over, not admitted; only a response says that.
    ///
    /// # Errors
    ///
    /// `BrokenPipe` once the call has ended, which also ends a wait on the ring
    /// of a daemon that died; any other error of the ring. The batch's fate is
    /// then unknown: resend it on a new call, as over gRPC. A daemon that already
    /// stored it reports it as a duplicate by its sequence numbers.
    pub async fn send(&self, batch: AppendSamplesRequest) -> io::Result<()> {
        if let Some(ring) = self
            .ring
            .as_ref()
            .filter(|_| self.plane() != DataPlaneKind::ControlPlane)
        {
            let frame = batch.encode_to_vec();
            if frame.len() <= ring.max_frame() {
                return tokio::select! {
                    sent = ring.send(&frame) => sent,
                    () = self.grpc.closed() => Err(gone()),
                };
            }
            self.left_ring.store(true, Ordering::Release);
        }
        self.grpc.send(batch).await.map_err(|_| gone())
    }
}
