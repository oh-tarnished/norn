//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One `AppendSamples` call's two loops: reading batches, and forwarding each
//! admitted batch's second response once it completes.

use norn_protocol::telemetry::AppendSamplesResponse;
use norn_runtime::ingest::Completion;
use tokio::sync::mpsc;
use tonic::Status;

use super::source::Source;
use super::{Telemetry, reply};

impl Telemetry {
    /// Reads batches until the client stops, or the daemon starts draining:
    /// first responses in batch order, each admitted batch's completion queued
    /// for [`forward`]. True when it stopped for the drain.
    ///
    /// A client may hold a call open between batches for as long as it likes,
    /// and a call in progress keeps its connection through HTTP/2's graceful
    /// GOAWAY. So without the drain arm an idle append held every shutdown
    /// until the drain bound cut it.
    pub(super) async fn read(
        self,
        mut input: Source,
        out: mpsc::Sender<Result<AppendSamplesResponse, Status>>,
        waits: mpsc::Sender<(AppendSamplesResponse, Completion)>,
    ) -> bool {
        let draining = self.ingest.draining();
        let mut last_batch = None;
        loop {
            let next = tokio::select! {
                biased;
                () = draining.cancelled() => return true,
                next = input.next() => next,
            };
            let (request, oversized, paid) = match next {
                Ok(Some(request)) => request,
                Ok(None) => return false,
                Err(status) => {
                    let _ = out.send(Err(status)).await;
                    return false;
                }
            };
            let full = waits.capacity() == 0;
            let (first, completion) = self.batch(request, oversized, full, &mut last_batch);
            // Decoded and decided: a ring frame's decoding is paid for no longer.
            drop(paid);
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
pub(super) async fn forward(
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
