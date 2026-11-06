//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the fake says, and the stream it says it on.

use crate::fake::Step;
use norn_protocol::telemetry::{AcknowledgementLevel as Ack, Admission, AppendSamplesResponse};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tonic::Status;
use tonic::codegen::tokio_stream::Stream;

pub fn reply(batch_id: i64, (outcome, reason, delay): Step, level: Ack) -> AppendSamplesResponse {
    AppendSamplesResponse {
        batch_id,
        admission: Some(Admission {
            outcome: outcome as i32,
            reason: reason as i32,
            retry_delay: delay.map(|ms| prost_types::Duration {
                seconds: 0,
                nanos: i32::try_from(ms * 1_000_000).unwrap_or(0),
            }),
            message: format!("{outcome:?}"),
        }),
        ack_level: level as i32,
        ..AppendSamplesResponse::default()
    }
}

/// What Norn ends an open call with as it drains to stop.
pub fn shutting_down() -> Status {
    use norn_protocol::rpc::{ErrorDetails, StatusExt};
    let details = ErrorDetails::with_error_info(
        "ERROR_REASON_SHUTTING_DOWN",
        "norn.local",
        std::collections::HashMap::new(),
    );
    Status::with_error_details(tonic::Code::Unavailable, "shutting down", details)
}

/// A receiver as a response stream.
pub struct Replies<T>(pub mpsc::Receiver<T>);

impl<T> Stream for Replies<T> {
    type Item = T;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.0.poll_recv(cx)
    }
}
