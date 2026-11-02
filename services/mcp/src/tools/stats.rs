//! get_statistics and compare_periods, per signal over [`summary`](super::summary).

use norn_protocol::google::r#type::Interval;
use serde_json::{Map, Value, json};

use super::args_more::{ComparePeriods, GetStatistics};
use super::parse;
use super::summary::{Reader, statistics};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::{Rows, start};
use crate::render;
use crate::select::{Selection, instant, interval, selection};
use crate::signal_scan::SignalScan;

/// The most signals one call summarises: each is six reads, twice for a comparison.
const MAX_SIGNALS: u32 = 50;
const DEFAULT_SIGNALS: u32 = 20;

/// Whether a failure is the deadline, with rows to return: that is a continuation.
fn out_of_time(e: &ToolError, rows: &Rows) -> bool {
    e.reason == "MCP_DEADLINE_EXCEEDED" && !rows.is_empty()
}

/// The tool.
pub(super) async fn get_statistics(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: GetStatistics = parse(raw)?;
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_SIGNALS, MAX_SIGNALS, cont)?;
    let chosen = selection(&args.target, args.select.as_deref())?;
    let window = interval(args.from.as_deref(), args.to.as_deref(), begin.now)?;
    let filter = args.sample_filter.as_deref().unwrap_or_default();
    let mut scan = SignalScan::new(call, chosen, begin.cursor, begin.limit);
    let mut rows = Rows::new(begin.limit);
    while let Some(signal) = scan.peek().await? {
        let reader = Reader {
            call,
            signal,
            interval: window,
            filter,
        };
        let (mut row, _) = match statistics(&reader).await {
            Ok(done) => done,
            Err(e) if out_of_time(&e, &rows) => return Ok(rows.finish(Some(scan.rest()))),
            Err(e) => return Err(e),
        };
        row.insert("signal".to_owned(), json!(signal.name));
        if !rows.push(Value::Object(row)) {
            return Ok(rows.finish(Some(scan.rest())));
        }
        scan.consume();
    }
    Ok(rows.finish(None))
}

fn change(a: &[(&str, f64)], b: &[(&str, f64)]) -> Value {
    let mut out = Map::new();
    for (name, before) in a {
        if let Some((_, after)) = b.iter().find(|(n, _)| n == name) {
            out.insert((*name).to_owned(), render::number_json(after - before));
        }
    }
    Value::Object(out)
}

fn bounds(p: &super::args_more::Period, now: i128) -> Result<Option<Interval>, ToolError> {
    Ok(Some(Interval {
        start_time: Some(instant("from", &p.from, now)?),
        end_time: Some(instant("to", &p.to, now)?),
    }))
}

/// The tool.
pub(super) async fn compare_periods(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: ComparePeriods = parse(raw)?;
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_SIGNALS, MAX_SIGNALS, cont)?;
    let chosen: Selection = selection(&args.target, args.select.as_deref())?;
    let (a, b) = (bounds(&args.a, begin.now)?, bounds(&args.b, begin.now)?);
    let filter = args.sample_filter.as_deref().unwrap_or_default();
    let mut scan = SignalScan::new(call, chosen, begin.cursor, begin.limit);
    let mut rows = Rows::new(begin.limit);
    while let Some(signal) = scan.peek().await? {
        let side = |interval| Reader {
            call,
            signal,
            interval,
            filter,
        };
        let (first, second) = (side(a), side(b));
        let both =
            async { Ok::<_, ToolError>((statistics(&first).await?, statistics(&second).await?)) };
        let ((row_a, numbers_a), (row_b, numbers_b)) = match both.await {
            Ok(done) => done,
            Err(e) if out_of_time(&e, &rows) => return Ok(rows.finish(Some(scan.rest()))),
            Err(e) => return Err(e),
        };
        let delta = change(&numbers_a, &numbers_b);
        let row = json!({"signal": signal.name, "a": row_a, "b": row_b, "change": delta});
        if !rows.push(row) {
            return Ok(rows.finish(Some(scan.rest())));
        }
        scan.consume();
    }
    Ok(rows.finish(None))
}
