//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-signal statistics over an interval, from the query engine's own reductions:
//! `COUNT`, `MIN`, `MAX`, `AVERAGE`, `SUM` and `LATEST`, each over a whole page of
//! signals at once, plus the first sample of each, whose time no reduction gives.
//!
//! These are several reads, not one snapshot: on a signal still receiving samples,
//! an interval reaching the present can see a sample in one figure and not another.

use std::collections::HashMap;
use std::sync::Arc;

use norn_core::{Timestamp, Value, ValueKind};
use norn_protocol::query::SignalStats;
use norn_runtime::query::{
    Aggregate, Engine, Filter, Function, Order, Output, QuerySpec, Reduction, Target, TimeRange,
};
use tonic::Status;

use super::pull::{Item, cannot, cut, each, open};
use crate::convert::{time_to_wire, value_to_wire};

/// Which samples one statistics read covers.
#[derive(Clone)]
pub(crate) struct Over {
    /// The interval.
    pub(crate) range: TimeRange,
    /// The sample filter.
    pub(crate) filter: Filter,
}

fn numeric(kind: ValueKind) -> bool {
    use ValueKind as K;
    matches!(kind, K::I32 | K::I64 | K::U32 | K::U64 | K::F32 | K::F64)
}

/// `function` over the whole interval for each of `targets`, by signal name.
fn reduce(
    engine: &Arc<Engine>,
    targets: Vec<Target>,
    over: &Over,
    function: Function,
) -> Result<HashMap<Arc<str>, Aggregate>, Status> {
    let mut found = HashMap::new();
    if targets.is_empty() {
        return Ok(found);
    }
    let spec = QuerySpec {
        targets,
        range: over.range,
        filter: over.filter.clone(),
        output: Output::Aggregates(Reduction {
            function,
            window: None,
        }),
        resume: None,
    };
    each(&mut open(engine, spec)?, |item| {
        if let Item::Aggregate(a) = item {
            found.insert(Arc::clone(&a.signal), a);
        }
        Ok(true)
    })?;
    Ok(found)
}

/// The earliest matching sample of `target`: the first result of a raw read, which
/// reads one engine page at most.
fn earliest(
    engine: &Arc<Engine>,
    target: &Target,
    over: &Over,
) -> Result<Option<(Timestamp, Value)>, Status> {
    let spec = QuerySpec {
        targets: vec![target.clone()],
        range: over.range,
        filter: over.filter.clone(),
        output: Output::Samples(Order::BySignal),
        resume: None,
    };
    let mut first = None;
    each(&mut open(engine, spec)?, |item| {
        if let Item::Sample(_, s) = item {
            first = Some((s.timestamp, s.value));
        }
        Ok(false)
    })?;
    Ok(first)
}

fn wire(
    value: &Value,
    longest: usize,
    cut_any: &mut bool,
) -> Result<norn_protocol::common::Value, Status> {
    let mut v = value_to_wire(value).map_err(cannot)?;
    *cut_any |= cut(Some(&mut v), longest);
    Ok(v)
}

fn double(a: Option<&Aggregate>) -> Option<f64> {
    match a.map(|a| &a.value) {
        Some(Value::F64(v)) => Some(*v),
        _ => None,
    }
}

/// The statistics of each of `targets`, in name order, values cut at `longest`.
/// Blocks on the disk and the query budget.
///
/// # Errors
/// Whatever a read fails with, such as `DATA_LOSS`.
pub(crate) fn statistics(
    engine: &Arc<Engine>,
    targets: &[Target],
    over: &Over,
    longest: usize,
) -> Result<Vec<SignalStats>, Status> {
    let all = || targets.to_vec();
    let counted = reduce(engine, all(), over, Function::Count)?;
    let some: Vec<Target> = targets
        .iter()
        .filter(|t| counted.get(&t.name).is_some_and(|a| a.samples > 0))
        .cloned()
        .collect();
    let numbers: Vec<Target> = some.iter().filter(|t| numeric(t.kind)).cloned().collect();
    let mins = reduce(engine, some.clone(), over, Function::Min)?;
    let maxes = reduce(engine, some.clone(), over, Function::Max)?;
    let latest = reduce(engine, some.clone(), over, Function::Latest)?;
    let averages = reduce(engine, numbers.clone(), over, Function::Average)?;
    let sums = reduce(engine, numbers, over, Function::Sum)?;
    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        let n = &t.name;
        let mut row = SignalStats {
            signal: n.to_string(),
            sample_count: counted
                .get(n)
                .map_or(0, |a| i64::try_from(a.samples).unwrap_or(i64::MAX)),
            ..SignalStats::default()
        };
        if row.sample_count > 0 {
            let mut cut_any = false;
            row.min_value = mins
                .get(n)
                .map(|a| wire(&a.value, longest, &mut cut_any))
                .transpose()?;
            row.max_value = maxes
                .get(n)
                .map(|a| wire(&a.value, longest, &mut cut_any))
                .transpose()?;
            row.average = double(averages.get(n));
            row.sum = double(sums.get(n));
            if let Some((time, value)) = earliest(engine, t, over)? {
                row.first_time = Some(time_to_wire(time));
                row.first_value = Some(wire(&value, longest, &mut cut_any)?);
            }
            if let Some(a) = latest.get(n) {
                row.last_time = a.start.map(time_to_wire);
                row.last_value = Some(wire(&a.value, longest, &mut cut_any)?);
            }
            row.value_truncated = cut_any;
        }
        out.push(row);
    }
    Ok(out)
}
