//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A `QuerySamplesRequest` as a `QuerySpec`.

use norn_core::Timestamp;
use norn_metadata::{Registry, SignalRecord};
use norn_protocol::common::ErrorReason;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest, SampleOrder};
use norn_runtime::query::{
    Filter, Function, Order, Output, Position, QuerySpec, Reduction, Target, TimeRange,
};
use prost::Message;
use tonic::{Code, Status};

use crate::catalog::select::select_signals;
use crate::convert::time_from_wire;
use crate::status::{bad_field, error_info};

/// The token format's name, changed if a position ever changes meaning.
const TOKEN: &str = "q1";

/// Identifies a request, its token aside, so a token cannot resume another query.
pub(super) fn fingerprint(request: &QuerySamplesRequest) -> u64 {
    let mut bare = request.clone();
    bare.resume_token.clear();
    print_of(&bare)
}

/// FNV-1a over `message` encoded: a fingerprint and not a secret. A caller clears
/// the fields a token may differ in (the token itself, a page size) first.
pub(crate) fn print_of(message: &impl Message) -> u64 {
    message
        .encode_to_vec()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// The token for `position` in a query of `print`.
pub(crate) fn token(print: u64, position: &Position) -> String {
    format!("{TOKEN}.{print:016x}.{}", position.encode())
}

/// The position `text`, a [`token`] of a query of `print`; `None` when empty.
///
/// # Errors
/// `INVALID_ARGUMENT` with `ERROR_REASON_INVALID_TOKEN` for any other text.
pub(crate) fn position(text: &str, print: u64) -> Result<Option<Position>, Status> {
    if text.is_empty() {
        return Ok(None);
    }
    let bad = || {
        let message = "the resume token does not match this request";
        error_info(
            Code::InvalidArgument,
            ErrorReason::InvalidToken,
            message,
            [],
        )
    };
    let rest = text.strip_prefix(&format!("{TOKEN}.{print:016x}."));
    rest.and_then(Position::parse).map(Some).ok_or_else(bad)
}

fn time(field: &str, t: Option<prost_types::Timestamp>) -> Result<Option<Timestamp>, Status> {
    t.map(|t| time_from_wire(t).map_err(|e| bad_field(field, &e.to_string())))
        .transpose()
}

/// `interval`, the request field `field`, as a query's range.
///
/// # Errors
/// `INVALID_ARGUMENT` for a time out of range or an interval ending before it starts.
pub(crate) fn time_range(interval: Option<Interval>, field: &str) -> Result<TimeRange, Status> {
    let (start, end) = interval.map_or((None, None), |i| (i.start_time, i.end_time));
    let range = TimeRange {
        start: time(&format!("{field}.start_time"), start)?,
        end: time(&format!("{field}.end_time"), end)?,
    };
    if let (Some(a), Some(b)) = (range.start, range.end)
        && a > b
    {
        return Err(bad_field(field, "must not end before it starts"));
    }
    Ok(range)
}

fn function(number: i32) -> Result<Function, Status> {
    Ok(match AggregateFunction::try_from(number) {
        Ok(AggregateFunction::Latest) => Function::Latest,
        Ok(AggregateFunction::First) => Function::First,
        Ok(AggregateFunction::Last) => Function::Last,
        Ok(AggregateFunction::Count) => Function::Count,
        Ok(AggregateFunction::Min) => Function::Min,
        Ok(AggregateFunction::Max) => Function::Max,
        Ok(AggregateFunction::Average) => Function::Average,
        Ok(AggregateFunction::Sum) => Function::Sum,
        Ok(AggregateFunction::Rate) => Function::Rate,
        _ => return Err(bad_field("aggregation.aggregate_function", "is required")),
    })
}

/// What `aggregation` and `order` ask a query to return.
///
/// # Errors
/// `INVALID_ARGUMENT` for an unknown order or function, or a bad window.
pub(crate) fn output(aggregation: Option<Aggregation>, order: i32) -> Result<Output, Status> {
    let Some(a) = aggregation else {
        return Ok(Output::Samples(match SampleOrder::try_from(order) {
            Ok(SampleOrder::ByTime) => Order::ByTime,
            Ok(_) => Order::BySignal,
            Err(_) => return Err(bad_field("order", "is not a SampleOrder")),
        }));
    };
    let window = a
        .window
        .map(|w| {
            let wide = i128::from(w.seconds) * 1_000_000_000 + i128::from(w.nanos);
            i64::try_from(wide).ok().filter(|n| *n > 0).ok_or_else(|| {
                bad_field("aggregation.window", "must be positive and fit 292 years")
            })
        })
        .transpose()?;
    Ok(Output::Aggregates(Reduction {
        function: function(a.aggregate_function)?,
        window,
    }))
}

/// `text` as a sample filter.
///
/// # Errors
/// `INVALID_ARGUMENT` naming `sample_filter` when it does not parse.
pub(crate) fn sample_filter(text: &str) -> Result<Filter, Status> {
    Filter::parse(text).map_err(|e| bad_field("sample_filter", &e.reason))
}

/// The signals `parent` and `filter` select, as a query's targets.
///
/// # Errors
/// `INVALID_ARGUMENT` for a malformed filter, `NOT_FOUND` for an unknown entity.
pub(crate) fn targets(
    registry: &Registry,
    parent: &str,
    filter: &str,
) -> Result<Vec<Target>, Status> {
    let selected = select_signals(registry, parent, filter)?;
    let target = |(entity, s): &(String, SignalRecord)| Target {
        name: format!("entities/{entity}/signals/{}", s.descriptor.name).into(),
        signal: s.descriptor.id,
        stream: s.stream,
        kind: s.descriptor.kind,
    };
    Ok(selected.iter().map(target).collect())
}

/// Resolves `request` against `registry`. Blocks on nothing but locks.
///
/// # Errors
/// `INVALID_ARGUMENT` for any malformed field, `NOT_FOUND` for an unknown entity.
pub(super) fn spec(
    registry: &Registry,
    request: QuerySamplesRequest,
    print: u64,
) -> Result<QuerySpec, Status> {
    Ok(QuerySpec {
        targets: targets(registry, &request.parent, &request.filter)?,
        range: time_range(request.interval, "interval")?,
        filter: sample_filter(&request.sample_filter)?,
        output: output(request.aggregation, request.order)?,
        resume: position(&request.resume_token, print)?,
    })
}
