//! Each tool's arguments. These types are the tools' input schemas, derived by
//! `schemars`, and their doc comments are what a model reads about each argument.

use schemars::JsonSchema;
use serde::Deserialize;

/// list_entities's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListEntities {
    /// Keep only entities matching this filter: `field = "value"` clauses joined by
    /// AND, over `display_name` and `annotations.<key>`.
    pub(crate) filter: Option<String>,
    /// The most rows to return: 1 to 1000, default 100.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on. Other arguments must not
    /// change, except `limit`.
    pub(crate) continuation: Option<String>,
}

/// list_signals's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListSignals {
    /// Whose signals: `all`, or an entity such as `entities/pump-1`.
    pub(crate) target: Option<String>,
    /// Keep only signals matching this filter: `field = "value"` clauses joined by AND,
    /// over `id`, `display_name`, `unit`, `stream`, `value_type` (such as
    /// `VALUE_TYPE_DOUBLE`) and `annotations.<key>`.
    pub(crate) filter: Option<String>,
    /// The most rows to return: 1 to 1000, default 100.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on.
    pub(crate) continuation: Option<String>,
}

/// get_signal_metadata's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetSignal {
    /// The signal's full name, such as `entities/pump-1/signals/flow`.
    pub(crate) signal: String,
}

/// An aggregate function.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Function {
    /// The first sample's value.
    First,
    /// The last sample's value.
    Last,
    /// How many samples.
    Count,
    /// The smallest value.
    Min,
    /// The largest value.
    Max,
    /// The mean. Numeric signals only.
    Average,
    /// The sum. Numeric signals only.
    Sum,
    /// A counter's increase per second; a decrease counts as a reset, so never use it
    /// on a value that may fall. Numeric signals only.
    Rate,
}

/// The order of raw samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SampleOrder {
    /// Each signal's samples in time order, signals by name. The cheaper order.
    Signal,
    /// All signals merged by time.
    Time,
}

/// query_range's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueryRange {
    /// `all`, an entity (`entities/pump-1`), or a signal
    /// (`entities/pump-1/signals/flow`).
    pub(crate) target: String,
    /// Narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// Start of the half-open interval, included: RFC 3339 in UTC such as
    /// `2026-11-02T08:00:00Z`, Unix seconds, `now`, or `now-1h`. Unset: from the beginning.
    pub(crate) from: Option<String>,
    /// End of the interval, excluded, in the same forms. Unset: to the newest sample.
    pub(crate) to: Option<String>,
    /// Keep only samples matching: clauses joined by AND over `value`,
    /// `quality.severity` and `quality.substatus`, with = != < <= > >=, such as
    /// `value > 20.5 AND quality.severity = QUALITY_SEVERITY_GOOD`. A literal the type
    /// of a selected signal cannot be compared with fails the call.
    #[serde(rename = "where")]
    pub(crate) sample_filter: Option<String>,
    /// Reduce each signal's samples with this function instead of returning them.
    pub(crate) aggregate: Option<Function>,
    /// With `aggregate`: one result per window of this width (`500ms`, `10s`, `5m`, `1h`,
    /// `1d`), aligned to `from`. Empty windows give no row. Unset: one over the interval.
    pub(crate) window: Option<String>,
    /// The order of raw samples. Default `signal`. Aggregates are always by signal, then window.
    pub(crate) order: Option<SampleOrder>,
    /// The most rows to return: 1 to 1000, default 100.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on.
    pub(crate) continuation: Option<String>,
}

/// get_latest's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetLatest {
    /// `all`, an entity, or a signal.
    pub(crate) target: String,
    /// Narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// Newest among samples matching this, as query_range's `where`.
    #[serde(rename = "where")]
    pub(crate) sample_filter: Option<String>,
    /// The most rows to return: 1 to 1000, default 100.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on.
    pub(crate) continuation: Option<String>,
}

/// state_at's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct StateAt {
    /// `all`, an entity, or a signal.
    pub(crate) target: String,
    /// The instant, in the forms query_range takes. A sample at exactly it counts.
    pub(crate) at: String,
    /// Narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// Report a sample older than this before `at` as unknown (`known: false`), such as `15m`.
    pub(crate) max_age: Option<String>,
    /// The most rows to return: 1 to 180, default 100.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result. It is valid for about a minute.
    pub(crate) continuation: Option<String>,
}
