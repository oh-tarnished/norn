//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The daemon's request decoding path, exposed for the fuzz crate only
//! (feature `fuzzing`). Not an API: no shipped build enables the feature.
//!
//! - [`read_body`] is the metering every gRPC call's body passes through:
//!   the prefix check, the size limit, the reservation, and for
//!   `AppendSamples` the sample count and the stripping of an oversized batch.
//! - [`ring_frame`] is a shared-memory frame's path: paid for, copied, counted
//!   and decoded.
//! - [`validate`] is what `AppendSamples` checks of a decoded batch before the
//!   registry is consulted: acknowledgement level, stream name, and every
//!   sample's conversion to the core's types.

use bytes::Bytes;
use norn_core::{Sample, SignalId};
use norn_protocol::telemetry::AppendSamplesRequest;
use norn_runtime::Budget;
use tokio::runtime::Handle;
use tonic::Status;

use crate::catalog::names;
use crate::convert::ConvertError;
use crate::metered::MeterLimits;
use crate::metered::drive;
use crate::telemetry::{arrive, batch};

/// The bounds the meter enforces: `[limits]`'s `max_batch_bytes` and
/// `max_batch_samples`.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The largest `AppendSamples` message, in bytes.
    pub max_batch_bytes: usize,
    /// The most samples one batch may carry.
    pub max_batch_samples: usize,
}

impl From<Limits> for MeterLimits {
    fn from(l: Limits) -> Self {
        Self {
            max_batch_bytes: l.max_batch_bytes,
            max_batch_samples: l.max_batch_samples,
        }
    }
}

/// One message as a handler receives it.
#[derive(Debug)]
pub struct Delivered {
    /// The message, gRPC prefix included, or the status the call fails with.
    pub result: Result<Bytes, Status>,
    /// Bytes reserved from the budget while it is decoded.
    pub reserved: u64,
    /// For a stripped `AppendSamples` batch, how many samples it had.
    pub oversized: Option<u64>,
}

/// Reads a body arriving as `chunks` through the meter, as `AppendSamples`
/// when `append`. Copies each message once, as the meter does.
pub async fn read_body(
    chunks: Vec<Bytes>,
    append: bool,
    budget: Budget,
    limits: Limits,
) -> Vec<Delivered> {
    let read = drive::read_body(chunks, append, budget, limits.into()).await;
    read.into_iter()
        .map(|d| Delivered {
            result: d.result,
            reserved: d.reserved,
            oversized: d.oversized,
        })
        .collect()
}

/// A decoded shared-memory frame: the batch, whether it was stripped, and the
/// bytes still reserved for it.
#[derive(Debug)]
pub struct Frame {
    /// The batch.
    pub request: AppendSamplesRequest,
    /// For a stripped batch, how many samples it had.
    pub oversized: Option<u64>,
    /// Bytes reserved for it.
    pub reserved: u64,
}

/// Takes and decodes one ring frame, as the ring's reader does. Must run
/// outside the runtime `handle` drives: it blocks on the budget.
///
/// # Errors
/// The status the call would fail the frame with.
pub fn ring_frame(
    frame: &[u8],
    budget: &Budget,
    limits: Limits,
    handle: &Handle,
) -> Result<Frame, Status> {
    let (payload, paid) = arrive::take(frame, budget, limits.into(), handle)?;
    let arriving = arrive::decode(payload, paid, limits.into())?;
    Ok(Frame {
        reserved: arriving.paid.bytes(),
        request: arriving.request,
        oversized: arriving.oversized,
    })
}

/// What `AppendSamples` makes of a decoded batch before the registry is
/// consulted: `Err` for a refusal of the whole batch, else each sample's
/// conversion, every one recorded as signal 0 in place of its route.
///
/// # Errors
/// The status the whole batch is refused with.
pub fn validate(
    request: AppendSamplesRequest,
    limits: Limits,
) -> Result<Vec<Result<Sample, ConvertError>>, Status> {
    if request.samples.len() > limits.max_batch_samples {
        return Err(Status::invalid_argument("too many samples"));
    }
    if batch::level(request.ack_level).is_none() {
        return Err(Status::invalid_argument("ack_level"));
    }
    names::stream(&request.stream)?;
    let signal = SignalId::new(0);
    let converted = request.samples.into_iter();
    Ok(converted
        .map(|s| batch::sample_from_wire(s, signal))
        .collect())
}
