//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Where a call's batches come from. Everything after [`Source::next`] is the
//! same code for either plane, which is what keeps their semantics equal.

use std::sync::Arc;

use norn_protocol::common::ErrorReason;
use norn_protocol::telemetry::AppendSamplesRequest;
use norn_runtime::Reservation;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tonic::{Code, Status, Streaming};

use super::pump::{Attached, Control, Item};
use crate::metered::Held;
use crate::status::error_info;

/// A batch, its oversized sample count, and what pays for its decoding (gRPC's
/// is held by the meter instead).
pub(crate) type Next = (AppendSamplesRequest, Option<u64>, Option<Reservation>);
type Sent = Result<(AppendSamplesRequest, Option<u64>), Status>;

/// A call's input.
#[derive(Debug)]
pub(crate) enum Source {
    /// The fallback, and the default: every batch is a gRPC message.
    Grpc(Box<Streaming<AppendSamplesRequest>>, Option<Arc<Held>>),
    /// A ring, with the gRPC side of the call still open beside it.
    Ring(Box<RingInput>),
}

/// A ring and the call that carries it. The call's request side says when the
/// client has finished, or died, and carries any batch too big for a frame.
#[derive(Debug)]
pub(crate) struct RingInput {
    frames: mpsc::Receiver<Item>,
    control: Arc<Control>,
    grpc: mpsc::Receiver<Sent>,
    forwarding: JoinHandle<()>,
    open: bool,
    pending: Option<(AppendSamplesRequest, Option<u64>)>,
    broken: Option<Status>,
}

impl Drop for RingInput {
    fn drop(&mut self) {
        self.control.stop();
        self.forwarding.abort();
    }
}

impl Source {
    /// A ring input. `input` is watched from a task of its own, which ends with
    /// the call.
    pub(crate) fn ring(
        ring: Attached,
        mut input: Streaming<AppendSamplesRequest>,
        held: Option<Arc<Held>>,
    ) -> Self {
        let (tx, grpc) = mpsc::channel(1);
        let forwarding = tokio::spawn(async move {
            loop {
                let next = input.message().await;
                let oversized = held.as_ref().and_then(|h| h.take_oversized());
                let sent = match next {
                    Ok(Some(request)) => Ok((request, oversized)),
                    Ok(None) => return,
                    Err(status) => Err(status),
                };
                let last = sent.is_err();
                if tx.send(sent).await.is_err() || last {
                    return;
                }
            }
        });
        Self::Ring(Box::new(RingInput {
            frames: ring.frames,
            control: ring.control,
            grpc,
            forwarding,
            open: true,
            pending: None,
            broken: None,
        }))
    }

    /// The next batch; `None` when the client has finished.
    pub(crate) async fn next(&mut self) -> Result<Option<Next>, Status> {
        match self {
            Self::Grpc(input, held) => {
                let request = input.message().await?;
                let oversized = held.as_ref().and_then(|h| h.take_oversized());
                Ok(request.map(|r| (r, oversized, None)))
            }
            Self::Ring(ring) => ring.next().await,
        }
    }
}

impl RingInput {
    /// A gRPC batch waits for the ring to empty: whatever the client published
    /// before sending it is in there, so the order is the client's. A broken call
    /// is a dead client: every whole frame it published is still answered, and
    /// what it was writing was never published, so nothing partial is admitted.
    async fn next(&mut self) -> Result<Option<Next>, Status> {
        loop {
            tokio::select! {
                item = self.frames.recv() => match item {
                    Some(Item::Batch(a)) => {
                        return Ok(Some((a.request, a.oversized, Some(a.paid))));
                    }
                    Some(Item::Flushed) => {
                        if let Some((r, oversized)) = self.pending.take() {
                            return Ok(Some((r, oversized, None)));
                        }
                    }
                    Some(Item::Failed(status)) => return Err(status),
                    Some(Item::Done) => return self.broken.take().map_or(Ok(None), Err),
                    None => {
                        let text = "the shared-memory reader stopped unexpectedly".to_owned();
                        let (code, reason) = (Code::Unavailable, ErrorReason::CapacityExhausted);
                        return Err(error_info(code, reason, text, []));
                    }
                },
                sent = self.grpc.recv(), if self.open && self.pending.is_none() => match sent {
                    Some(Ok(next)) => {
                        self.pending = Some(next);
                        self.control.flush();
                    }
                    Some(Err(status)) => {
                        self.broken = Some(status);
                        self.open = false;
                        self.control.stop();
                    }
                    None => {
                        self.open = false;
                        self.control.stop();
                    }
                },
            }
        }
    }
}
