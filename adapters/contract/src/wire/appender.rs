//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One `AppendSamples` call, one batch in flight: a bounded client by construction.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use norn_protocol::common::Sample;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{
    AcknowledgementLevel, AppendSamplesRequest, AppendSamplesResponse, BackpressureOutcome,
};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};
use tonic::Streaming;
use tonic::codegen::tokio_stream;
use tonic::transport::Channel;

use super::refused::refused;
use crate::AdapterError;

const METHOD: &str = "TelemetryService.AppendSamples";

/// A bounded channel as the request stream, so a slow daemon stalls the sender.
struct Batches(mpsc::Receiver<AppendSamplesRequest>);

impl tokio_stream::Stream for Batches {
    type Item = AppendSamplesRequest;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// Sends a batch and waits for it, retrying only what the daemon said may be
/// retried. Waiting is the backpressure: memory stays at one batch.
pub(crate) struct Appender {
    tx: mpsc::Sender<AppendSamplesRequest>,
    replies: Streaming<AppendSamplesResponse>,
    level: AcknowledgementLevel,
    next_id: i64,
    not_before: Option<Instant>,
    max_retries: u32,
    pub(crate) duplicates: u64,
    pub(crate) batches: u64,
    pub(crate) throttled: u64,
    pub(crate) retried: u64,
}

impl Appender {
    pub(crate) async fn open(
        channel: Channel,
        level: AcknowledgementLevel,
        max_retries: u32,
    ) -> Result<Self, AdapterError> {
        let (tx, rx) = mpsc::channel(1);
        let opened = TelemetryServiceClient::new(channel)
            .append_samples(Batches(rx))
            .await;
        let replies = opened.map_err(AdapterError::call(METHOD))?.into_inner();
        Ok(Self {
            tx,
            replies,
            level,
            next_id: 0,
            not_before: None,
            max_retries,
            duplicates: 0,
            batches: 0,
            throttled: 0,
            retried: 0,
        })
    }

    /// Sends one batch until it reaches the level, or the daemon refuses it.
    pub(crate) async fn send(
        &mut self,
        stream: &str,
        samples: Vec<Sample>,
    ) -> Result<(), AdapterError> {
        let mut retries = 0;
        loop {
            if let Some(when) = self.not_before.take() {
                sleep_until(when).await;
            }
            self.next_id += 1;
            let request = AppendSamplesRequest {
                stream: stream.to_owned(),
                batch_id: self.next_id,
                // Cloned because a transient refusal resends it.
                samples: samples.clone(),
                ack_level: self.level as i32,
            };
            if self.tx.send(request).await.is_err() {
                return Err(self.broken().await);
            }
            let Some(delay) = self.settled(&samples).await? else {
                self.batches += 1;
                return Ok(());
            };
            retries += 1;
            self.retried += 1;
            if retries > self.max_retries {
                return Err(AdapterError::LimitExceeded {
                    limit: "max_retries",
                    value: u64::from(self.max_retries),
                });
            }
            self.not_before = Some(Instant::now() + delay);
        }
    }

    /// Reads the batch's responses: `None` once it reached the level, or the delay
    /// to retry after.
    async fn settled(&mut self, samples: &[Sample]) -> Result<Option<Duration>, AdapterError> {
        let mut duplicates = 0;
        loop {
            let Some(reply) = self.next_reply().await? else {
                return Err(AdapterError::Response {
                    method: METHOD,
                    detail: "an answer to the last batch; the call ended",
                });
            };
            let admission = reply.admission.clone().unwrap_or_default();
            let delay = admission.retry_delay.map(|d| {
                Duration::new(
                    d.seconds.max(0).unsigned_abs(),
                    d.nanos.max(0).unsigned_abs(),
                )
            });
            let outcome = BackpressureOutcome::try_from(admission.outcome);
            if outcome == Ok(BackpressureOutcome::Reject) {
                if let Some(delay) = delay {
                    return Ok(Some(delay));
                }
                return Err(refused(&reply, samples));
            }
            if reply.error.is_some() {
                return Err(refused(&reply, samples));
            }
            if outcome == Ok(BackpressureOutcome::Throttle)
                && let Some(delay) = delay
                && self.not_before.is_none()
            {
                self.throttled += 1;
                self.not_before = Some(Instant::now() + delay);
            }
            let reached = AcknowledgementLevel::try_from(reply.ack_level);
            let reached = reached.unwrap_or(AcknowledgementLevel::Unspecified);
            // The first response of a batch carries the count; a second may too.
            duplicates = duplicates.max(reply.duplicate_count.max(0).unsigned_abs());
            if reached as i32 >= self.level as i32 {
                self.duplicates += duplicates;
                return Ok(None);
            }
        }
    }

    async fn next_reply(&mut self) -> Result<Option<AppendSamplesResponse>, AdapterError> {
        (self.replies.message().await).map_err(AdapterError::call(METHOD))
    }

    /// Why the request side stopped: the call's own status, if it has one.
    async fn broken(&mut self) -> AdapterError {
        match self.next_reply().await {
            Err(error) => error,
            Ok(_) => AdapterError::Response {
                method: METHOD,
                detail: "an open request stream; the call ended",
            },
        }
    }

    /// Ends the call cleanly: everything sent has already been acknowledged.
    pub(crate) async fn close(mut self) -> Result<(), AdapterError> {
        drop(self.tx);
        while self
            .replies
            .message()
            .await
            .map_err(AdapterError::call(METHOD))?
            .is_some()
        {}
        Ok(())
    }
}
