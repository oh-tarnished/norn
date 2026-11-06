//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Driving a [`Reader`] over a body held in memory, for the fuzz crate only
//! (feature `fuzzing`): the same reader, limits and reservations the
//! [`super::Meter`] layer puts in front of every gRPC call.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, PoisonError};
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
use norn_runtime::Budget;
use tonic::Status;
use tonic::body::Body;

use super::reader::Reader;
use super::{Held, MeterLimits};

/// A request body arriving as these chunks, as HTTP/2 DATA frames would.
struct Chunks(VecDeque<Bytes>);

impl HttpBody for Chunks {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Poll::Ready(self.get_mut().0.pop_front().map(|b| Ok(Frame::data(b))))
    }
}

/// One message as the handler would receive it.
#[derive(Debug)]
pub(crate) struct Delivered {
    /// The message with its gRPC prefix, or the status the call fails with.
    pub(crate) result: Result<Bytes, Status>,
    /// Bytes reserved from the budget while the handler decodes it.
    pub(crate) reserved: u64,
    /// The sample count of an `AppendSamples` batch stripped for having too many.
    pub(crate) oversized: Option<u64>,
}

/// Reads every message of a body made of `chunks`, metered as `AppendSamples`
/// when `append`, otherwise as any other method.
pub(crate) async fn read_body(
    chunks: Vec<Bytes>,
    append: bool,
    budget: Budget,
    limits: MeterLimits,
) -> Vec<Delivered> {
    let held = Arc::new(Held::default());
    let body = Body::new(Chunks(chunks.into()));
    let mut reader = Reader::new(body, budget, limits, append, &held);
    let mut out = Vec::new();
    loop {
        let (next, step) = reader.next_frame().await;
        reader = next;
        let result = match step {
            None => break,
            Some(Ok(frame)) => match frame.into_data() {
                Ok(data) => Ok(data),
                Err(_) => continue,
            },
            Some(Err(status)) => Err(status),
        };
        let failed = result.is_err();
        let memory = held.memory.lock().unwrap_or_else(PoisonError::into_inner);
        let reserved = memory.as_ref().map_or(0, norn_runtime::Reservation::bytes);
        drop(memory);
        let oversized = held.take_oversized();
        out.push(Delivered {
            result,
            reserved,
            oversized,
        });
        if failed {
            break;
        }
    }
    out
}
