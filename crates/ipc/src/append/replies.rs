//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Responses keep the publisher alive while the daemon drains its frames.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use norn_protocol::telemetry::AppendSamplesResponse;
use tonic::codegen::tokio_stream::Stream;
use tonic::{Status, Streaming};

use crate::DataPlane;

/// The responses to an [`AppendCall`](super::AppendCall), all over gRPC.
///
/// Keeps a shared-memory publisher alive after the sender closes: dropping a
/// publisher before the daemon connects to it can discard unread frames. Drain
/// these replies to finish the call; ending or dropping them releases that hold.
pub struct AppendReplies {
    replies: Streaming<AppendSamplesResponse>,
    ring: Option<Arc<dyn DataPlane>>,
}

impl std::fmt::Debug for AppendReplies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppendReplies").finish_non_exhaustive()
    }
}

impl AppendReplies {
    pub(super) fn new(
        replies: Streaming<AppendSamplesResponse>,
        ring: Option<Arc<dyn DataPlane>>,
    ) -> Self {
        Self { replies, ring }
    }

    /// The next response, or `None` once every batch has been answered.
    ///
    /// # Errors
    ///
    /// The call's gRPC status.
    pub async fn message(&mut self) -> Result<Option<AppendSamplesResponse>, Status> {
        poll_fn(|cx| Pin::new(&mut *self).poll_next(cx))
            .await
            .transpose()
    }
}

impl Stream for AppendReplies {
    type Item = Result<AppendSamplesResponse, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = Pin::new(&mut self.replies).poll_next(cx);
        if matches!(next, Poll::Ready(None | Some(Err(_)))) {
            self.ring = None;
        }
        next
    }
}
