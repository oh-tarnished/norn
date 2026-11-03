//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `ComputeStats` and `ComparePeriods`: [`statistics`] over a page of signals.
//!
//! A page token names the last signal answered (`t1.<fingerprint>.<signal>`), so a
//! signal created between calls cannot shift a page, and the fingerprint binds the
//! token to its request.

use std::sync::Arc;

use norn_metadata::Registry;
use norn_protocol::common::ErrorReason;
use norn_protocol::common::{Value as WireValue, value::Kind};
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{
    ComparePeriodsRequest, ComparePeriodsResponse, ComputeStatsRequest, ComputeStatsResponse,
    PeriodComparison, SignalStats, StatsChange,
};
use norn_runtime::query::{Engine, Target};
use tonic::{Code, Status};

use super::pull::{page_size, value_bytes};
use super::request::{print_of, sample_filter, targets, time_range};
use super::stats::{Over, statistics};
use crate::status::{bad_field, error_info};

/// The default and largest page, in signals: each is a few reads, twice for a
/// comparison.
const DEFAULT_PAGE: i32 = 20;
const MAX_PAGE: usize = 50;
const PREFIX: &str = "t1";

/// The page of `parent`'s signals matching `filter` after `token`, and whether more
/// follow.
fn page(
    registry: &Registry,
    (parent, filter): (&str, &str),
    (token, print): (&str, u64),
    asked: i32,
) -> Result<(Vec<Target>, bool), Status> {
    let limit = page_size("page_size", asked, DEFAULT_PAGE as usize, MAX_PAGE)?;
    let after = match token {
        "" => None,
        t => Some(
            t.strip_prefix(&format!("{PREFIX}.{print:016x}."))
                .ok_or_else(|| {
                    let message = "the page token does not match this request";
                    error_info(
                        Code::InvalidArgument,
                        ErrorReason::InvalidToken,
                        message,
                        [],
                    )
                })?,
        ),
    };
    let mut all = targets(registry, parent, filter)?;
    all.sort_by(|a, b| a.name.cmp(&b.name));
    let rest = all
        .into_iter()
        .filter(|t| after.is_none_or(|a| &*t.name > a));
    let mut chosen: Vec<Target> = rest.take(limit + 1).collect();
    let more = chosen.len() > limit;
    chosen.truncate(limit);
    Ok((chosen, more))
}

fn next(print: u64, chosen: &[Target], more: bool) -> String {
    match chosen.last() {
        Some(t) if more => format!("{PREFIX}.{print:016x}.{}", t.name),
        _ => String::new(),
    }
}

/// Answers `ComputeStats`. Blocks on the disk and the query budget.
///
/// # Errors
/// `INVALID_ARGUMENT` for a malformed field or token; whatever a read fails with.
pub(super) fn compute(
    registry: &Registry,
    engine: &Arc<Engine>,
    request: ComputeStatsRequest,
) -> Result<ComputeStatsResponse, Status> {
    let longest = value_bytes(request.max_value_bytes)?;
    let mut bare = request.clone();
    bare.page_token.clear();
    bare.page_size = 0;
    bare.max_value_bytes = 0;
    let print = print_of(&bare);
    let over = Over {
        range: time_range(request.interval, "interval")?,
        filter: sample_filter(&request.sample_filter)?,
    };
    let selection = (request.parent.as_str(), request.filter.as_str());
    let token = (request.page_token.as_str(), print);
    let (chosen, more) = page(registry, selection, token, request.page_size)?;
    Ok(ComputeStatsResponse {
        signal_stats: statistics(engine, &chosen, &over, longest)?,
        next_page_token: next(print, &chosen, more),
    })
}

/// A closed interval, both ends required.
fn closed(interval: Option<Interval>, field: &str) -> Result<Option<Interval>, Status> {
    match interval {
        Some(i) if i.start_time.is_some() && i.end_time.is_some() => Ok(Some(i)),
        _ => Err(bad_field(field, "needs both a start_time and an end_time")),
    }
}

/// A figure as a number, where it is one.
fn number(v: Option<&WireValue>) -> Option<f64> {
    #[allow(clippy::cast_precision_loss, reason = "IEEE arithmetic, as documented")]
    Some(match v?.kind.as_ref()? {
        Kind::Int32Value(n) => f64::from(*n),
        Kind::Int64Value(n) | Kind::Uint32Value(n) => *n as f64,
        // The wire carries a uint64's bits in an int64 (protocol.md, "Unsigned values").
        Kind::Uint64Value(n) => u64::from_ne_bytes(n.to_ne_bytes()) as f64,
        Kind::FloatValue(n) => f64::from(*n),
        Kind::DoubleValue(n) => *n,
        _ => return None,
    })
}

fn change(a: &SignalStats, b: &SignalStats) -> StatsChange {
    let both = |x: Option<f64>, y: Option<f64>| Some(y? - x?);
    StatsChange {
        sample_count: b.sample_count.saturating_sub(a.sample_count),
        min_value: both(number(a.min_value.as_ref()), number(b.min_value.as_ref())),
        max_value: both(number(a.max_value.as_ref()), number(b.max_value.as_ref())),
        average: both(a.average, b.average),
        sum: both(a.sum, b.sum),
    }
}

/// Answers `ComparePeriods`. Blocks on the disk and the query budget.
///
/// # Errors
/// `INVALID_ARGUMENT` for a malformed field or token; whatever a read fails with.
pub(super) fn compare(
    registry: &Registry,
    engine: &Arc<Engine>,
    request: ComparePeriodsRequest,
) -> Result<ComparePeriodsResponse, Status> {
    let longest = value_bytes(request.max_value_bytes)?;
    let mut bare = request.clone();
    bare.page_token.clear();
    bare.page_size = 0;
    bare.max_value_bytes = 0;
    let print = print_of(&bare);
    let filter = sample_filter(&request.sample_filter)?;
    let side = |interval, field: &str| -> Result<Over, Status> {
        let range = time_range(closed(interval, field)?, field)?;
        Ok(Over {
            range,
            filter: filter.clone(),
        })
    };
    let baseline = side(request.baseline_interval, "baseline_interval")?;
    let comparison = side(request.comparison_interval, "comparison_interval")?;
    let selection = (request.parent.as_str(), request.filter.as_str());
    let token = (request.page_token.as_str(), print);
    let (chosen, more) = page(registry, selection, token, request.page_size)?;
    let a = statistics(engine, &chosen, &baseline, longest)?;
    let b = statistics(engine, &chosen, &comparison, longest)?;
    let rows = a.into_iter().zip(b).map(|(a, b)| PeriodComparison {
        signal: a.signal.clone(),
        change: Some(change(&a, &b)),
        baseline: Some(a),
        comparison: Some(b),
    });
    Ok(ComparePeriodsResponse {
        period_comparisons: rows.collect(),
        next_page_token: next(print, &chosen, more),
    })
}
