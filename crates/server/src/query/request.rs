//! A `QuerySamplesRequest` as a `QuerySpec`.

use norn_core::Timestamp;
use norn_metadata::{Registry, SignalRecord};
use norn_protocol::common::ErrorReason;
use norn_protocol::query::{AggregateFunction, QuerySamplesRequest, SampleOrder};
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
/// FNV-1a over the encoded message; a fingerprint and not a secret.
pub(super) fn fingerprint(request: &QuerySamplesRequest) -> u64 {
    let mut bare = request.clone();
    bare.resume_token.clear();
    bare.encode_to_vec()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// The token for `position` in a query of `print`.
pub(super) fn token(print: u64, position: &Position) -> String {
    format!("{TOKEN}.{print:016x}.{}", position.encode())
}

fn position(text: &str, print: u64) -> Result<Option<Position>, Status> {
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

fn range(request: &QuerySamplesRequest) -> Result<TimeRange, Status> {
    let (start, end) = request
        .interval
        .map_or((None, None), |i| (i.start_time, i.end_time));
    let range = TimeRange {
        start: time("interval.start_time", start)?,
        end: time("interval.end_time", end)?,
    };
    if let (Some(a), Some(b)) = (range.start, range.end)
        && a > b
    {
        return Err(bad_field("interval", "must not end before it starts"));
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

fn output(request: &QuerySamplesRequest) -> Result<Output, Status> {
    let Some(a) = request.aggregation else {
        return Ok(Output::Samples(
            match SampleOrder::try_from(request.order) {
                Ok(SampleOrder::ByTime) => Order::ByTime,
                Ok(_) => Order::BySignal,
                Err(_) => return Err(bad_field("order", "is not a SampleOrder")),
            },
        ));
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
    let filter =
        Filter::parse(&request.sample_filter).map_err(|e| bad_field("sample_filter", &e.reason))?;
    Ok(QuerySpec {
        targets: targets(registry, &request.parent, &request.filter)?,
        range: range(&request)?,
        filter,
        output: output(&request)?,
        resume: position(&request.resume_token, print)?,
    })
}
