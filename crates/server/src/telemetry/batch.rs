//! Deciding about one batch: validate whole, then admit whole.

use norn_core::{Sample, Sequence};
use norn_protocol::common::{self, ErrorReason};
use norn_protocol::telemetry::{
    AcknowledgementLevel, AppendSamplesRequest, BackpressureReason, SampleError,
};
use norn_runtime::ingest::{AckLevel, Batch};
use tonic::Code;

use super::{Handled, Telemetry, reply};
use crate::catalog::names;
use crate::convert::{ConvertError, quality_from_wire, time_from_wire, value_from_wire};
use crate::status::{bad_field, error_info, to_rpc};

/// A sample's fields, converted.
fn sample_from_wire(
    s: common::Sample,
    signal: norn_core::SignalId,
) -> Result<Sample, ConvertError> {
    if s.sequence < 0 {
        return Err(ConvertError::NegativeSequence(s.sequence));
    }
    let time = s
        .observe_time
        .ok_or(ConvertError::Missing("observe_time"))?;
    let value = s.value.ok_or(ConvertError::Missing("value"))?;
    let quality = s.quality.ok_or(ConvertError::Missing("quality"))?;
    Ok(Sample::new(
        time_from_wire(time)?,
        signal,
        value_from_wire(value)?,
        quality_from_wire(quality)?,
        Sequence::new(s.sequence as u64),
    ))
}

fn sample_error(index: usize, code: Code, reason: ErrorReason, text: String) -> SampleError {
    let status = error_info(code, reason, text, []);
    SampleError {
        index: i32::try_from(index).unwrap_or(i32::MAX),
        status: Some(to_rpc(&status)),
    }
}

fn level(number: i32) -> Option<AckLevel> {
    Some(match AcknowledgementLevel::try_from(number).ok()? {
        AcknowledgementLevel::Received => AckLevel::Received,
        AcknowledgementLevel::Buffered => AckLevel::Buffered,
        AcknowledgementLevel::Persisted => AckLevel::Persisted,
        AcknowledgementLevel::Durable => AckLevel::Durable,
        AcknowledgementLevel::Unspecified => return None,
    })
}

impl Telemetry {
    /// Decides about `request`, whole. Synchronous: it reads memory, takes short
    /// locks and never waits on a disk.
    pub(super) fn batch(
        &self,
        request: AppendSamplesRequest,
        oversized: Option<u64>,
        pending_full: bool,
        last_batch: &mut Option<i64>,
    ) -> Handled {
        let id = request.batch_id;
        let invalid = BackpressureReason::InvalidSamples;
        if last_batch.is_some_and(|last| id <= last) {
            let status = bad_field("batch_id", "must increase within a call");
            return (reply::refused(id, invalid, None, &status), None);
        }
        *last_batch = Some(id);
        if let Some(count) = oversized.or_else(|| {
            let n = request.samples.len();
            (n > self.max_batch_samples).then_some(n as u64)
        }) {
            let limit = self.max_batch_samples;
            let text = format!("{count} samples exceeds max_batch_samples, {limit}");
            let meta = [("limit", "max_batch_samples".to_owned())];
            let status = error_info(
                Code::InvalidArgument,
                ErrorReason::LimitExceeded,
                text,
                meta,
            );
            let too_large = BackpressureReason::BatchTooLarge;
            return (reply::refused(id, too_large, None, &status), None);
        }
        if pending_full {
            let limit = self.max_pending;
            let text = format!("{limit} batches are already unacknowledged on this call");
            let meta = [("limit", "max_pending_batches".to_owned())];
            let status = error_info(
                Code::ResourceExhausted,
                ErrorReason::LimitExceeded,
                text,
                meta,
            );
            let full = BackpressureReason::BufferFull;
            let delay = Some(prost_types::Duration {
                seconds: 0,
                nanos: 250_000_000,
            });
            return (reply::refused(id, full, delay, &status), None);
        }
        let Some(wanted) = level(request.ack_level) else {
            let status = bad_field("ack_level", "is required");
            return (reply::refused(id, invalid, None, &status), None);
        };
        if request.samples.is_empty() {
            let status = bad_field("samples", "is required");
            return (reply::refused(id, invalid, None, &status), None);
        }
        let stream_id = match names::stream(&request.stream) {
            Ok(s) => s.to_owned(),
            Err(status) => return (reply::refused(id, invalid, None, &status), None),
        };
        let view = self.registry.view();
        let Some(stream) = view.stream(&stream_id) else {
            let meta = [("resource", request.stream.clone())];
            let text = format!("{} does not exist", request.stream);
            let status = error_info(Code::NotFound, ErrorReason::ResourceNotFound, text, meta);
            return (reply::refused(id, invalid, None, &status), None);
        };
        let stream = stream.descriptor.id;
        let mut samples = Vec::with_capacity(request.samples.len());
        let mut routes = Vec::with_capacity(request.samples.len());
        let mut errors = Vec::new();
        for (index, wire) in request.samples.into_iter().enumerate() {
            let Some(route) = view.route(&wire.signal) else {
                let text = format!("signal {} does not exist", wire.signal);
                let e = sample_error(index, Code::NotFound, ErrorReason::SignalNotFound, text);
                errors.push(e);
                continue;
            };
            let code = Code::InvalidArgument;
            if route.stream != stream {
                let text = format!("{} records into another stream", wire.signal);
                errors.push(sample_error(
                    index,
                    code,
                    ErrorReason::InvalidArgument,
                    text,
                ));
                continue;
            }
            match sample_from_wire(wire, route.id) {
                Ok(s) if s.value.kind() == route.kind => {
                    samples.push(s);
                    routes.push(route);
                }
                Ok(s) => {
                    let (got, want) = (s.value.kind(), route.kind);
                    let text = format!("value is {got:?}; the signal records {want:?}");
                    errors.push(sample_error(
                        index,
                        code,
                        ErrorReason::ValueTypeMismatch,
                        text,
                    ));
                }
                Err(e) => {
                    let text = e.to_string();
                    errors.push(sample_error(
                        index,
                        code,
                        ErrorReason::InvalidArgument,
                        text,
                    ));
                }
            }
        }
        if !errors.is_empty() {
            return (reply::invalid_samples(id, errors), None);
        }
        let admission = self.ingest.admit(Batch::new(stream, samples, wanted));
        if admission.reached.is_some() {
            routes.iter().for_each(|r| r.mark_data());
        }
        drop(view);
        reply::decided(id, admission)
    }
}
