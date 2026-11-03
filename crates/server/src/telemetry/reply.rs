//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Building responses: runtime decisions in protocol terms.

use std::time::Duration;

use norn_protocol::common::ErrorReason;
use norn_protocol::telemetry::{
    AcknowledgementLevel, Admission as WireAdmission, AppendSamplesResponse, BackpressureOutcome,
    BackpressureReason, SampleError,
};
use norn_runtime::ingest::{AckFailure, AckLevel, Admission, Decision, Outcome, Reason};
use tokio::sync::oneshot::error::RecvError;
use tonic::{Code, Status};

use super::Handled;
use crate::status::{error_info, to_rpc};

fn level_to_wire(level: AckLevel) -> i32 {
    let wire = match level {
        AckLevel::Received => AcknowledgementLevel::Received,
        AckLevel::Buffered => AcknowledgementLevel::Buffered,
        AckLevel::Persisted => AcknowledgementLevel::Persisted,
        AckLevel::Durable => AcknowledgementLevel::Durable,
    };
    wire as i32
}

fn outcome_to_wire(outcome: Outcome) -> BackpressureOutcome {
    match outcome {
        Outcome::Accept => BackpressureOutcome::Accept,
        Outcome::Buffer => BackpressureOutcome::Buffer,
        Outcome::Throttle => BackpressureOutcome::Throttle,
        Outcome::Reject => BackpressureOutcome::Reject,
    }
}

fn reason_to_wire(reason: Reason) -> BackpressureReason {
    match reason {
        Reason::CapacityAvailable => BackpressureReason::CapacityAvailable,
        Reason::WritePathBehind => BackpressureReason::WritePathBehind,
        Reason::BufferHighWater => BackpressureReason::BufferHighWater,
        Reason::BufferFull => BackpressureReason::BufferFull,
        Reason::MemoryExhausted => BackpressureReason::MemoryExhausted,
        Reason::Throttled => BackpressureReason::Throttled,
        Reason::ShuttingDown => BackpressureReason::ShuttingDown,
        Reason::StorageUnavailable => BackpressureReason::StorageUnavailable,
        Reason::BatchTooLarge => BackpressureReason::BatchTooLarge,
        Reason::AckLevelUnavailable => BackpressureReason::AckLevelUnavailable,
        _ => BackpressureReason::Unspecified,
    }
}

fn duration(d: Duration) -> prost_types::Duration {
    prost_types::Duration {
        seconds: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        nanos: i32::try_from(d.subsec_nanos()).unwrap_or(0),
    }
}

fn admission_to_wire(d: &Decision) -> WireAdmission {
    WireAdmission {
        outcome: outcome_to_wire(d.outcome) as i32,
        reason: reason_to_wire(d.reason) as i32,
        retry_delay: d.retry_delay.map(duration),
        message: d.message.clone(),
    }
}

/// The error a rejection from the write path carries.
fn rejection(d: &Decision) -> Status {
    let (code, reason) = match d.reason {
        Reason::ShuttingDown => (Code::Unavailable, ErrorReason::ShuttingDown),
        Reason::StorageUnavailable => (Code::Unavailable, ErrorReason::StorageUnavailable),
        Reason::BatchTooLarge => (Code::InvalidArgument, ErrorReason::LimitExceeded),
        Reason::AckLevelUnavailable => {
            (Code::FailedPrecondition, ErrorReason::CapabilityUnavailable)
        }
        _ => (Code::ResourceExhausted, ErrorReason::CapacityExhausted),
    };
    error_info(code, reason, d.message.clone(), [])
}

/// The response for a batch refused before reaching the write path.
pub(super) fn refused(
    batch_id: i64,
    reason: BackpressureReason,
    retry_delay: Option<prost_types::Duration>,
    status: &Status,
) -> AppendSamplesResponse {
    AppendSamplesResponse {
        batch_id,
        admission: Some(WireAdmission {
            outcome: BackpressureOutcome::Reject as i32,
            reason: reason as i32,
            retry_delay,
            message: status.message().to_owned(),
        }),
        error: Some(to_rpc(status)),
        ..AppendSamplesResponse::default()
    }
}

/// The response for a batch with invalid samples, listing every one.
pub(super) fn invalid_samples(batch_id: i64, errors: Vec<SampleError>) -> AppendSamplesResponse {
    let text = format!("{} samples are invalid", errors.len());
    let status = error_info(
        Code::InvalidArgument,
        ErrorReason::InvalidArgument,
        text,
        [],
    );
    let mut response = refused(batch_id, BackpressureReason::InvalidSamples, None, &status);
    response.sample_errors = errors;
    response
}

/// The first response for a decided batch, and its completion if one follows.
pub(super) fn decided(batch_id: i64, admission: Admission) -> Handled {
    let mut response = AppendSamplesResponse {
        batch_id,
        admission: Some(admission_to_wire(&admission.decision)),
        duplicate_count: i64::try_from(admission.duplicates).unwrap_or(i64::MAX),
        ..AppendSamplesResponse::default()
    };
    match admission.reached {
        Some(reached) => response.ack_level = level_to_wire(reached),
        None => response.error = Some(to_rpc(&rejection(&admission.decision))),
    }
    (response, admission.completion)
}

/// The second response, from the first and how the batch ended.
pub(super) fn completed(
    first: AppendSamplesResponse,
    result: Result<Result<AckLevel, AckFailure>, RecvError>,
) -> AppendSamplesResponse {
    let mut second = first;
    let failure = match result {
        Ok(Ok(level)) => {
            second.ack_level = level_to_wire(level);
            return second;
        }
        Ok(Err(failure)) => failure.to_string(),
        Err(_) => "the stream stopped before the batch reached its level".to_owned(),
    };
    let status = error_info(
        Code::Unavailable,
        ErrorReason::StorageUnavailable,
        failure,
        [],
    );
    second.error = Some(to_rpc(&status));
    second
}
