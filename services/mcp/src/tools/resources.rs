//! list_entities, list_signals and get_signal_metadata.

use norn_protocol::common::ValueType;
use norn_protocol::entity::{Entity, ListEntitiesRequest};
use norn_protocol::signal::{GetSignalRequest, Signal};
use serde_json::{Value, json};

use super::{args as a, parse};
use crate::cursor::{Cursor, Key, Order};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::{DEFAULT_LIMIT, MAX_LIMIT, Rows, start};
use crate::render::when;
use crate::select::selection;
use crate::signal_scan::SignalScan;

fn entity_row(e: &Entity) -> Value {
    json!({
        "name": e.name,
        "display_name": e.display_name,
        "annotations": e.annotations,
    })
}

/// The name of a signal's value type, without its `VALUE_TYPE_` prefix.
pub(super) fn value_type(s: &Signal) -> &'static str {
    let kind = ValueType::try_from(s.value_type).unwrap_or(ValueType::Unspecified);
    kind.as_str_name().trim_start_matches("VALUE_TYPE_")
}

fn signal_row(s: &Signal) -> Value {
    json!({
        "name": s.name,
        "display_name": s.display_name,
        "value_type": value_type(s),
        "unit": s.unit,
        "stream": s.stream,
        "annotations": s.annotations,
    })
}

/// The tool. A daemon page and a result are bounded differently, so a result can stop
/// inside a page; [`Cursor`] then names that page and the last entity given.
pub(super) async fn list_entities(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: a::ListEntities = parse(raw)?;
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_LIMIT, MAX_LIMIT, cont)?;
    let mut rows = Rows::new(begin.limit);
    let cursor = begin.cursor;
    let mut token = cursor.r.clone();
    let mut last = None;
    loop {
        let request = ListEntitiesRequest {
            page_size: begin.limit as i32,
            page_token: token.clone(),
            filter: args.filter.clone().unwrap_or_default(),
        };
        let mut client = call.entities();
        let page = client.list_entities(request);
        let page = call.rpc("EntityService.ListEntities", page).await?;
        for entity in &page.entities {
            let key = Key::name(&entity.name);
            if cursor.given(&key, Order::Signal) {
                continue;
            }
            if !rows.push(entity_row(entity)) {
                let next = Cursor {
                    r: token,
                    a: last.or(cursor.a),
                    ..cursor
                };
                return Ok(rows.finish(Some(next)));
            }
            last = Some(key);
        }
        if page.next_page_token.is_empty() {
            return Ok(rows.finish(None));
        }
        token = page.next_page_token;
    }
}

/// The tool.
pub(super) async fn list_signals(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: a::ListSignals = parse(raw)?;
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_LIMIT, MAX_LIMIT, cont)?;
    let target = args.target.as_deref().unwrap_or("all");
    let chosen = selection(target, args.filter.as_deref())?;
    let mut scan = SignalScan::new(call, chosen, begin.cursor, begin.limit);
    let mut rows = Rows::new(begin.limit);
    while let Some(signal) = scan.peek().await? {
        if !rows.push(signal_row(signal)) {
            return Ok(rows.finish(Some(scan.rest())));
        }
        scan.consume();
    }
    Ok(rows.finish(None))
}

/// The tool.
pub(super) async fn get_signal(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: a::GetSignal = parse(raw)?;
    let mut client = call.signals();
    let get = client.get_signal(GetSignalRequest { name: args.signal });
    let s = call.rpc("SignalService.GetSignal", get).await?;
    let mut row = signal_row(&s);
    crate::render::set(&mut row, "create_time", when(s.create_time));
    crate::render::set(&mut row, "update_time", when(s.update_time));
    Ok(row)
}
