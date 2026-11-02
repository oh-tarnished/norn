//! query_range and get_latest.

use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest, SampleOrder};
use serde_json::{Value, json};

use super::args::{self as a, Function};
use super::parse;
use crate::cursor::Order;
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::{DEFAULT_LIMIT, MAX_LIMIT, Rows, start};
use crate::render;
use crate::select::{interval, selection, wire_duration};
use crate::walk::{Item, walk};

fn function(f: Function) -> AggregateFunction {
    match f {
        Function::First => AggregateFunction::First,
        Function::Last => AggregateFunction::Last,
        Function::Count => AggregateFunction::Count,
        Function::Min => AggregateFunction::Min,
        Function::Max => AggregateFunction::Max,
        Function::Average => AggregateFunction::Average,
        Function::Sum => AggregateFunction::Sum,
        Function::Rate => AggregateFunction::Rate,
    }
}

fn row(item: &Item) -> Value {
    match item {
        Item::Sample(s) => render::sample(s),
        Item::Aggregate(a) => render::aggregate(a),
    }
}

fn aggregation(args: &a::QueryRange) -> Result<Option<Aggregation>, ToolError> {
    let window = args.window.as_deref();
    let window = window.map(|w| wire_duration("window", w)).transpose()?;
    match args.aggregate {
        Some(f) => Ok(Some(Aggregation {
            aggregate_function: function(f) as i32,
            window,
        })),
        None if window.is_some() => Err(ToolError::invalid("window", "needs `aggregate`")),
        None => Ok(None),
    }
}

/// The tool.
pub(super) async fn query_range(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: a::QueryRange = parse(raw)?;
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_LIMIT, MAX_LIMIT, cont)?;
    let chosen = selection(&args.target, args.select.as_deref())?;
    let aggregation = aggregation(&args)?;
    if aggregation.is_some() && args.order == Some(a::SampleOrder::Time) {
        return Err(ToolError::invalid(
            "order",
            "aggregates are always by signal",
        ));
    }
    let by_time = args.order == Some(a::SampleOrder::Time);
    let request = QuerySamplesRequest {
        parent: chosen.parent,
        filter: chosen.filter,
        interval: interval(args.from.as_deref(), args.to.as_deref(), begin.now)?,
        sample_filter: args.sample_filter.clone().unwrap_or_default(),
        aggregation,
        order: if by_time {
            SampleOrder::ByTime
        } else {
            SampleOrder::BySignal
        } as i32,
        resume_token: String::new(),
    };
    let order = if by_time { Order::Time } else { Order::Signal };
    let mut rows = Rows::new(begin.limit);
    let next = walk(call, request, order, &begin.cursor, |item| {
        rows.push(row(&item))
    })
    .await?;
    Ok(rows.finish(next))
}

fn latest_row(item: &Item) -> Value {
    let Item::Aggregate(a) = item else {
        return Value::Null;
    };
    let (value, cut) = render::value(a.value.as_ref());
    let time = render::when(a.window.unwrap_or_default().start_time);
    let mut row = json!({"signal": a.signal, "time": time, "value": value});
    if cut {
        render::set(&mut row, "truncated", json!(true));
    }
    row
}

/// The tool: `LATEST` over all history, so nothing here depends on a clock.
pub(super) async fn get_latest(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: a::GetLatest = parse(raw)?;
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_LIMIT, MAX_LIMIT, cont)?;
    let chosen = selection(&args.target, args.select.as_deref())?;
    let request = QuerySamplesRequest {
        parent: chosen.parent,
        filter: chosen.filter,
        sample_filter: args.sample_filter.clone().unwrap_or_default(),
        aggregation: Some(Aggregation {
            aggregate_function: AggregateFunction::Latest as i32,
            window: None,
        }),
        ..QuerySamplesRequest::default()
    };
    let mut rows = Rows::new(begin.limit);
    let next = walk(call, request, Order::Signal, &begin.cursor, |item| {
        rows.push(latest_row(&item))
    })
    .await?;
    Ok(rows.finish(next))
}
