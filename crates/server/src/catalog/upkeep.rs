//! `StreamService`'s long-running upkeep: compact, verify, apply retention.

use norn_core::{StreamId, Timestamp};
use norn_metadata::Retention;
use norn_protocol::google::longrunning::Operation;
use norn_protocol::google::r#type::Interval;
use norn_protocol::stream::RetentionPolicy;
use norn_protocol::stream::retention_policy::Limit;
use norn_runtime::maintenance::Request as Job;
use tonic::Status;

use super::{Catalog, metadata_status, names};
use crate::convert::time_from_wire;
use crate::operations::{refused, to_wire};
use crate::status::bad_field;

/// A policy off the wire, whole or refused.
pub(super) fn retention_from_wire(policy: Option<RetentionPolicy>) -> Result<Retention, Status> {
    match policy.and_then(|p| p.limit) {
        None => Ok(Retention::Forever),
        Some(Limit::MaxAge(age)) => std::time::Duration::try_from(age)
            .ok()
            .filter(|d| !d.is_zero())
            .map(Retention::MaxAge)
            .ok_or_else(|| bad_field("retention.max_age", "must be positive")),
        Some(Limit::MaxBytes(bytes)) => u64::try_from(bytes)
            .ok()
            .filter(|n| *n > 0)
            .map(Retention::MaxBytes)
            .ok_or_else(|| bad_field("retention.max_bytes", "must be positive")),
        Some(Limit::MaxMonths(months)) => u32::try_from(months)
            .ok()
            .filter(|n| *n > 0)
            .map(Retention::MaxMonths)
            .ok_or_else(|| bad_field("retention.max_months", "must be positive")),
    }
}

/// A policy for the wire; `None` keeps forever.
pub(super) fn retention_to_wire(r: Retention) -> Option<RetentionPolicy> {
    let limit = match r {
        Retention::MaxAge(age) => Limit::MaxAge(age.try_into().ok()?),
        Retention::MaxBytes(n) => Limit::MaxBytes(i64::try_from(n).unwrap_or(i64::MAX)),
        Retention::MaxMonths(n) => Limit::MaxMonths(i32::try_from(n).unwrap_or(i32::MAX)),
        _ => return None,
    };
    Some(RetentionPolicy { limit: Some(limit) })
}

/// A half-open wire interval as inclusive bounds; an unset end is unbounded.
pub(super) fn bounds(interval: Option<Interval>) -> Result<Option<(Timestamp, Timestamp)>, Status> {
    let Some(i) = interval else { return Ok(None) };
    let time = |t, field| match t {
        Some(t) => time_from_wire(t)
            .map(Some)
            .map_err(|_| bad_field(field, "is out of range")),
        None => Ok(None),
    };
    let from = time(i.start_time, "interval.start_time")?.unwrap_or(Timestamp::MIN);
    let to = time(i.end_time, "interval.end_time")?.map_or(Timestamp::MAX, |t| {
        Timestamp::from_nanos(t.as_nanos().saturating_sub(1))
    });
    if from > to {
        return Err(bad_field("interval", "must not end before it starts"));
    }
    Ok(Some((from, to)))
}

impl Catalog {
    /// The stream named `name`.
    fn stream_id(&self, name: &str) -> Result<StreamId, Status> {
        let record = self.registry.stream(names::stream(name)?);
        Ok(record.map_err(|e| metadata_status(&e))?.descriptor.id)
    }

    /// Queues `job` on the stream named `name` and returns its operation.
    pub(super) fn start(&self, name: &str, job: Job) -> Result<Operation, Status> {
        let stream = self.stream_id(name)?;
        let op = self.maintenance.submit(stream, job).map_err(refused)?;
        Ok(to_wire(&op))
    }
}
