//! What a query asks for, once a caller has resolved names to signals.

use std::sync::Arc;

use norn_core::{SignalId, StreamId, Timestamp, ValueKind};

use crate::filter::Filter;
use crate::token::Position;

/// One selected signal. Selecting signals is the catalogue's job (entity →
/// signals is `norn-metadata`'s), so a query starts from these.
///
/// Concurrency: `Clone` shares the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The signal's full resource name, the order results are grouped in.
    pub name: Arc<str>,
    /// Its compact id.
    pub signal: SignalId,
    /// The stream it records into.
    pub stream: StreamId,
    /// Its value kind.
    pub kind: ValueKind,
}

/// A half-open interval of `observe_time`. Either end may be open.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeRange {
    /// The earliest timestamp included; `None` for all of the past.
    pub start: Option<Timestamp>,
    /// The first timestamp excluded; `None` for no limit.
    pub end: Option<Timestamp>,
}

impl TimeRange {
    /// The same range as inclusive bounds, or `None` when it holds no instant.
    pub(crate) fn inclusive(self) -> Option<(Timestamp, Timestamp)> {
        let from = self.start.unwrap_or(Timestamp::MIN);
        let to = match self.end {
            Some(end) => Timestamp::from_nanos(end.as_nanos().checked_sub(1)?),
            None => Timestamp::MAX,
        };
        (from <= to).then_some((from, to))
    }
}

/// The order of raw samples.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Order {
    /// Each signal's samples in time order, signals in name order. Cheapest.
    #[default]
    BySignal,
    /// All signals merged by time, then sequence, then signal name.
    ByTime,
}

/// A reduction; docs/reference/query.md defines each exactly.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Function {
    /// The newest matching sample of each signal; one result per signal.
    Latest,
    /// The window's earliest sample's value.
    First,
    /// The window's latest sample's value.
    Last,
    /// How many samples.
    Count,
    /// The smallest value.
    Min,
    /// The largest value.
    Max,
    /// The arithmetic mean.
    Average,
    /// The sum.
    Sum,
    /// A counter's increase per second.
    Rate,
}

impl Function {
    /// Whether it needs a numeric signal.
    pub(crate) fn numeric(self) -> bool {
        matches!(self, Self::Average | Self::Sum | Self::Rate)
    }
}

/// A function over windows of `window` nanoseconds; `None` is one window over the
/// whole interval.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reduction {
    /// The function.
    pub function: Function,
    /// The window width, positive. Ignored by [`Function::Latest`].
    pub window: Option<i64>,
}

/// What a query returns.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Output {
    /// The matching samples themselves.
    Samples(Order),
    /// One reduction per signal and window.
    Aggregates(Reduction),
}

/// One query.
///
/// Concurrency: owned data, `Send + Sync`.
#[derive(Debug, Clone)]
pub struct QuerySpec {
    /// The signals, any order; the engine sorts them by name.
    pub targets: Vec<Target>,
    /// The interval.
    pub range: TimeRange,
    /// Which samples count.
    pub filter: Filter,
    /// What to return.
    pub output: Output,
    /// Continue just after this result.
    pub resume: Option<Position>,
}
