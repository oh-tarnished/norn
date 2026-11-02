//! state_at: `ComputeState`, a page at a time.
//!
//! Unlike the other reads, its continuation is the daemon's own page token and never
//! re-reads: a token dies with the last page it names, because the daemon pins an
//! answer only until that page has been read. So a page must fit one result whole,
//! which [`MAX_STATE_LIMIT`] guarantees.

use norn_protocol::state::{ComputeStateRequest, SignalState};
use serde_json::{Value, json};

use super::{args as a, parse};
use crate::cursor::Cursor;
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::{DEFAULT_LIMIT, MAX_RESULT_BYTES, Rows, start};
use crate::render;
use crate::select::{instant, selection, wire_duration};

/// The most a row can take, name and JSON keys included: [`render::MAX_VALUE_CHARS`] of
/// value, about 150 of signal name, and about 250 of the rest.
const MAX_STATE_ROW: usize = render::MAX_VALUE_CHARS + 400;
/// The most rows a page may hold: it fits one result whole, whatever the values.
pub(crate) const MAX_STATE_LIMIT: u32 = 180;
const _: () = assert!(MAX_STATE_LIMIT as usize * MAX_STATE_ROW <= MAX_RESULT_BYTES);

fn row(entry: &SignalState) -> Value {
    match &entry.sample {
        Some(sample) => {
            let mut row = render::sample(sample);
            render::set(&mut row, "known", json!(true));
            row
        }
        None => json!({"signal": entry.signal, "known": false}),
    }
}

/// The tool.
pub(super) async fn state_at(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: a::StateAt = parse(raw)?;
    let cont = args.continuation.as_deref();
    let default = DEFAULT_LIMIT.min(MAX_STATE_LIMIT);
    let begin = start(call, raw, args.limit, default, MAX_STATE_LIMIT, cont)?;
    let chosen = selection(&args.target, args.select.as_deref())?;
    let max_age = args.max_age.as_deref();
    let request = ComputeStateRequest {
        parent: chosen.parent,
        filter: chosen.filter,
        state_time: Some(instant("at", &args.at, begin.now)?),
        max_age: max_age.map(|d| wire_duration("max_age", d)).transpose()?,
        page_size: begin.limit as i32,
        page_token: begin.cursor.r.clone(),
    };
    let mut client = call.state();
    let page = client.compute_state(request);
    let page = call.rpc("StateService.ComputeState", page).await?;
    let mut rows = Rows::new(begin.limit);
    for entry in &page.signal_states {
        rows.push(row(entry));
    }
    let next = (!page.next_page_token.is_empty()).then(|| Cursor {
        r: page.next_page_token.clone(),
        ..begin.cursor
    });
    let mut result = rows.finish(next);
    render::set(&mut result, "state_time", render::when(page.state_time));
    Ok(result)
}
