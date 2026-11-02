//! The tool list a client sees. Descriptions are written for a model: what the tool
//! answers, when to use it, and how its result is bounded.

use std::any::Any;

use rmcp::handler::server::tool::schema_for_type;
use rmcp::model::{Tool, ToolAnnotations};
use schemars::JsonSchema;

use super::{args as a, args_more as m};

fn tool<T: JsonSchema + Any>(
    name: &'static str,
    description: &'static str,
    idempotent: bool,
) -> Tool {
    let hints = ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(idempotent)
        .open_world(false);
    let mut tool = Tool::new(name, description, schema_for_type::<T>());
    tool.annotations = Some(hints);
    tool
}

/// Every tool this server offers. All of them only read history; none writes to Norn.
#[must_use]
pub fn tools() -> Vec<Tool> {
    vec![
        tool::<a::ListEntities>(
            "list_entities",
            "List the entities Norn records: the things that have signals. Start here to \
             learn what exists. Rows: name, display_name, annotations. Bounded: follow \
             `continuation` while `complete` is false.",
            true,
        ),
        tool::<a::ListSignals>(
            "list_signals",
            "List signals, of one entity or all: the measurable properties that have \
             history. Rows: name, display_name, value_type, unit, stream, annotations. Use the \
             full `name` as a `target` elsewhere. Bounded: follow `continuation` while \
             `complete` is false.",
            true,
        ),
        tool::<a::GetSignal>(
            "get_signal_metadata",
            "Read one signal's metadata: its type, unit, stream and annotations, and when it \
             was created. Says nothing of its samples: use get_latest or get_statistics.",
            true,
        ),
        tool::<a::QueryRange>(
            "query_range",
            "Read a signal's history over a time interval, as raw samples or reduced by an \
             aggregate over fixed windows. Filter by value and quality with `where`. Prefer an \
             aggregate with a `window` to reading many raw samples. A damaged chunk is an \
             error, never a gap. Bounded: 1000 rows and 256 KiB a call, and when `complete` is \
             false, repeat the call with `continuation` to read on.",
            true,
        ),
        tool::<a::GetLatest>(
            "get_latest",
            "The newest sample of each selected signal: what is happening now, or as of the \
             last report. Rows: signal, time, value; no quality, so use state_at for that, or \
             `where` to ask for the newest GOOD sample. A signal with no matching sample gives \
             no row. Bounded: follow `continuation` while `complete` is false.",
            true,
        ),
        tool::<a::StateAt>(
            "state_at",
            "What every selected signal held at one instant: each one's latest sample at or \
             before it. Every selected signal has a row, with `known: false` if it had no \
             sample by then. Use it for 'what did the system look like at T' and, with a time \
             just before an event, 'what led to it'. Bounded: 180 rows a call; pages of one \
             answer agree with each other, and `continuation` lasts about a minute.",
            true,
        ),
        tool::<m::FindEvents>(
            "find_events",
            "Find the moments something happened: samples matching a predicate (`match`), a \
             value changing (`value_change`), or quality changing (`quality_change`). \
             Deterministic: nothing is inferred, and an event is exactly what the kind says. \
             Rows are grouped by signal, in time order, and a change carries `previous`. A \
             signal's first sample in the interval is the baseline, never a change. Scans at \
             most 1,000,000 samples a call, so `complete: false` can arrive with few rows or \
             none: follow `continuation`.",
            true,
        ),
        tool::<m::GetStatistics>(
            "get_statistics",
            "Summarise each selected signal over an interval: sample_count, min, max, first and \
             last with their times, and for numeric signals average and sum. Each signal costs \
             several reads, so select narrowly. Bounded: 50 signals a call; follow \
             `continuation` while `complete` is false.",
            true,
        ),
        tool::<m::ComparePeriods>(
            "compare_periods",
            "Compare two intervals, such as this week with last: for each selected signal, \
             the get_statistics of period `a` and period `b`, and `change`, which is b minus a \
             for sample_count, min, max, average and sum where both are numeric. Computed from \
             aggregates, never from raw samples sent here. Bounded: 50 signals a call; follow \
             `continuation` while `complete` is false.",
            true,
        ),
        tool::<m::Replay>(
            "replay",
            "Walk through history in order, the way it happened, on a virtual clock that only \
             you move. `start` gives a session paused at `from`. `step` emits the next `events`, \
             or moves the clock by `duration`; `seek` jumps, and reports each signal's state just \
             before the target; `status` reads the session. Always `stop` when finished: \
             sessions are limited, and an abandoned one lasts until the daemon's idle timeout. \
             To read a whole interval use query_range; use replay to go step by step, as around \
             an incident. Reads history only.",
            false,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paged_note_is_the_contract_every_description_states() {
        let bounded = "complete";
        for tool in tools().iter().filter(|t| {
            t.name != "get_signal_metadata" && t.name != "replay" && t.name != "state_at"
        }) {
            let text = tool.description.as_deref().unwrap_or_default();
            assert!(text.contains(bounded), "{}", tool.name);
        }
    }

    #[test]
    fn every_tool_has_an_object_schema_refusing_unknown_arguments() {
        for tool in tools() {
            assert_eq!(
                tool.input_schema.get("type").and_then(|t| t.as_str()),
                Some("object")
            );
            let closed = tool.input_schema.get("additionalProperties");
            assert_eq!(closed, Some(&serde_json::json!(false)), "{}", tool.name);
        }
    }
}
