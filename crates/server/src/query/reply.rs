//! Pages and failures as the wire's.

use norn_protocol::common::ErrorReason;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{Aggregate as WireAggregate, QuerySamplesResponse};
use norn_runtime::query::{Aggregate, Charge, Page, QueryError, Refused, Results};
use tonic::{Code, Status};

use super::request::token;
use crate::convert::{ConvertError, sample_to_wire, time_to_wire, value_to_wire};
use crate::status::{bad_field, error_info};

fn aggregate(a: Aggregate) -> Result<WireAggregate, ConvertError> {
    Ok(WireAggregate {
        signal: a.signal.to_string(),
        window: Some(Interval {
            start_time: a.start.map(time_to_wire),
            end_time: a.end.map(time_to_wire),
        }),
        value: Some(value_to_wire(&a.value)?),
        sample_count: i64::try_from(a.samples).unwrap_or(i64::MAX),
    })
}

/// One response. A stored value the wire cannot carry, such as a sequence above
/// `i64::MAX`, fails the query with `INTERNAL`, naming no sample silently away.
pub(super) fn page(page: Page, print: u64) -> Result<(QuerySamplesResponse, Charge), Status> {
    let Page {
        results,
        position,
        charge,
        ..
    } = page;
    let mut out = QuerySamplesResponse {
        resume_token: token(print, &position),
        ..QuerySamplesResponse::default()
    };
    let cannot = |e: ConvertError| Status::internal(format!("cannot send a stored value: {e}"));
    match results {
        Results::Samples(list) => {
            let wire = list
                .iter()
                .map(|(name, s)| sample_to_wire(s, name.to_string()));
            out.samples = wire.collect::<Result<_, _>>().map_err(cannot)?;
        }
        Results::Aggregates(list) => {
            let wire = list.into_iter().map(aggregate);
            out.aggregates = wire.collect::<Result<_, _>>().map_err(cannot)?;
        }
        _ => return Err(Status::internal("a result of an unknown kind")),
    }
    Ok((out, charge))
}

/// A query failure as the protocol's status.
pub(crate) fn status(e: &QueryError) -> Status {
    let text = e.to_string();
    let reason = |code, reason, meta: Vec<(&'static str, String)>| {
        error_info(code, reason, text.clone(), meta)
    };
    match e {
        QueryError::Filter(f) => bad_field("sample_filter", &f.reason),
        QueryError::NotApplicable { what, .. } => {
            let field = if what.starts_with("sample_filter") {
                "sample_filter"
            } else {
                "aggregation.aggregate_function"
            };
            bad_field(field, &text)
        }
        QueryError::BadPosition => reason(Code::InvalidArgument, ErrorReason::InvalidToken, vec![]),
        QueryError::Corrupt(d) => {
            let chunk = format!(
                "streams/{}/{}@{}",
                d.stream,
                d.segment.file_name(),
                d.offset
            );
            let meta = vec![
                ("chunk", chunk),
                ("signal_id", d.signal.to_string()),
                ("min_time", d.min_ts.as_nanos().to_string()),
                ("max_time", d.max_ts.as_nanos().to_string()),
            ];
            reason(Code::DataLoss, ErrorReason::DataCorrupted, meta)
        }
        QueryError::Memory(Refused::Closed) => {
            reason(Code::Unavailable, ErrorReason::ShuttingDown, vec![])
        }
        QueryError::Memory(_) => {
            let meta = vec![("limit", "query_bytes".to_owned())];
            reason(
                Code::ResourceExhausted,
                ErrorReason::CapacityExhausted,
                meta,
            )
        }
        QueryError::TooLarge => {
            let meta = vec![("limit", "query_bytes".to_owned())];
            reason(Code::ResourceExhausted, ErrorReason::LimitExceeded, meta)
        }
        QueryError::Raced => reason(Code::Unavailable, ErrorReason::StorageUnavailable, vec![]),
        _ => reason(Code::Internal, ErrorReason::StorageUnavailable, vec![]),
    }
}
