//! The arguments of the tools that compute: find_events, the statistics and replay.

use schemars::JsonSchema;
use serde::Deserialize;

/// What counts as an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventKind {
    /// Every sample matching `where`, which is required.
    Match,
    /// Every sample whose value differs, bit for bit, from the signal's previous sample.
    ValueChange,
    /// Every sample whose quality differs from the signal's previous sample.
    QualityChange,
}

/// find_events's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FindEvents {
    /// `all`, an entity, or a signal.
    pub(crate) target: String,
    /// What counts as an event.
    pub(crate) kind: EventKind,
    /// Start of the half-open interval, included, in the forms query_range takes.
    pub(crate) from: Option<String>,
    /// End of the interval, excluded.
    pub(crate) to: Option<String>,
    /// Narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// A sample filter, as query_range's `where`. Required for `match`. With a change
    /// kind, only matching samples are compared, each with the previous matching one.
    #[serde(rename = "where")]
    pub(crate) sample_filter: Option<String>,
    /// The most events to return: 1 to 1000, default 100.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on.
    pub(crate) continuation: Option<String>,
}

/// get_statistics's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetStatistics {
    /// `all`, an entity, or a signal.
    pub(crate) target: String,
    /// Start of the half-open interval, included, in the forms query_range takes.
    pub(crate) from: Option<String>,
    /// End of the interval, excluded.
    pub(crate) to: Option<String>,
    /// Narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// Count only samples matching this, as query_range's `where`.
    #[serde(rename = "where")]
    pub(crate) sample_filter: Option<String>,
    /// The most signals to return: 1 to 50, default 20.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on.
    pub(crate) continuation: Option<String>,
}

/// One period of a comparison.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Period {
    /// Start, included, in the forms query_range takes.
    pub(crate) from: String,
    /// End, excluded.
    pub(crate) to: String,
}

/// compare_periods's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComparePeriods {
    /// `all`, an entity, or a signal.
    pub(crate) target: String,
    /// The baseline period.
    pub(crate) a: Period,
    /// The period compared with it. `change` in the result is b minus a.
    pub(crate) b: Period,
    /// Narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// Count only samples matching this, as query_range's `where`.
    #[serde(rename = "where")]
    pub(crate) sample_filter: Option<String>,
    /// The most signals to return: 1 to 50, default 20.
    pub(crate) limit: Option<u32>,
    /// The `continuation` of the previous result, to read on.
    pub(crate) continuation: Option<String>,
}

/// What to do with a replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ReplayAction {
    /// Create a session paused at `from`. Needs `target`, `from`, `to`.
    Start,
    /// Emit the next events. Needs `session`, and `events` or `duration`.
    Step,
    /// Move the clock. Needs `session`, `time`. Returns each signal's state just before it.
    Seek,
    /// Read the session. Needs `session`.
    Status,
    /// End the session and free it. Needs `session`. Always do this when finished.
    Stop,
    /// Name this server's open sessions.
    List,
}

/// replay's arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Replay {
    /// What to do.
    pub(crate) action: ReplayAction,
    /// `start`: `all`, an entity, or a signal.
    pub(crate) target: Option<String>,
    /// `start`: narrow the target with a signal filter, as list_signals takes.
    pub(crate) select: Option<String>,
    /// `start`: where the replay begins, in the forms query_range takes.
    pub(crate) from: Option<String>,
    /// `start`: where it ends, excluded.
    pub(crate) to: Option<String>,
    /// The session's name, from `start`.
    pub(crate) session: Option<String>,
    /// `step`: how many sample events to emit, at most `limit`.
    pub(crate) events: Option<u32>,
    /// `step`: how far to move the clock, such as `10s`. Refused if that would pass `limit`.
    pub(crate) duration: Option<String>,
    /// `seek`: the new clock, within the session's interval.
    pub(crate) time: Option<String>,
    /// The most events one call returns: 1 to 1000, default 100.
    pub(crate) limit: Option<u32>,
}
