//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A frame out of a ring, made into a batch: paid for, copied, counted, decoded,
//! with the gRPC path's own arithmetic (`metered::cost`).

use std::time::Duration;

use bytes::Bytes;
use norn_protocol::common::ErrorReason;
use norn_protocol::telemetry::AppendSamplesRequest;
use norn_runtime::{BudgetError, Reservation};
use prost::Message;
use tokio::runtime::Handle;
use tonic::{Code, Status};

use super::plane::Plane;
use crate::metered::{PREFIX, Settled, settle_batch, strip_samples, worst_batch_cost};
use crate::status::error_info;

/// How long a frame waits for decode budget, as a gRPC message does.
const RESERVE_BOUND: Duration = Duration::from_secs(30);

/// A batch out of a ring: decoded, validated as protobuf, paid for.
#[derive(Debug)]
pub(crate) struct Arriving {
    pub(super) request: AppendSamplesRequest,
    pub(super) oversized: Option<u64>,
    pub(super) paid: Reservation,
}

pub(super) fn invalid(text: String) -> Status {
    error_info(
        Code::InvalidArgument,
        ErrorReason::InvalidArgument,
        text,
        [],
    )
}

/// The status a socket call gets when its message cannot be paid for.
fn no_room(e: &BudgetError) -> Status {
    let (code, reason) = match e {
        BudgetError::Closed { .. } => (Code::Unavailable, ErrorReason::ShuttingDown),
        _ => (Code::ResourceExhausted, ErrorReason::CapacityExhausted),
    };
    error_info(code, reason, e.to_string(), [("limit", "ipc_bytes".into())])
}

/// Pays for a frame, then copies it out. The copy is the only thing ever looked
/// at again: the peer can rewrite the original while it is read. Blocks the
/// reader's thread while the budget is short, which leaves the frame in its
/// slot: the ring fills and the producer waits, as HTTP/2 flow control would.
pub(super) fn take(
    frame: &[u8],
    plane: &Plane,
    handle: &Handle,
) -> Result<(Bytes, Reservation), Status> {
    let worst = worst_batch_cost(frame.len(), plane.limits);
    let pool = plane.budget.capacity() / 2;
    let reserve = plane.budget.reserve(worst.min(pool).max(1), RESERVE_BOUND);
    let paid = handle.block_on(reserve).map_err(|e| no_room(&e))?;
    Ok((Bytes::copy_from_slice(frame), paid))
}

/// Counts and decodes a copied frame. A batch with too many samples is decoded
/// without them, so the handler refuses it by its `batch_id`, as on gRPC.
pub(super) fn decode(
    payload: Bytes,
    mut paid: Reservation,
    plane: &Plane,
) -> Result<Arriving, Status> {
    let settled = settle_batch(&payload, &mut paid, plane.limits);
    let (bytes, oversized) = match settled {
        Settled::Fits => (payload, None),
        Settled::Oversized(n) => (strip_samples(&payload).slice(PREFIX..), Some(n)),
    };
    let request = AppendSamplesRequest::decode(bytes)
        .map_err(|e| invalid(format!("a shared-memory frame is not a batch: {e}")))?;
    Ok(Arriving {
        request,
        oversized,
        paid,
    })
}
