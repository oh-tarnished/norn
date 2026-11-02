//! What a plan is asked for.

use std::sync::Arc;

use norn_core::{SignalId, StreamId, Timestamp};

/// Which signals a plan covers.
///
/// Invariant: an explicit set is sorted and free of duplicates, so a membership
/// test is O(log n) and pages are visited once, in order.
///
/// Concurrency: `Send + Sync`; cloning shares the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalFilter(Option<Arc<[SignalId]>>);

impl SignalFilter {
    /// Every signal.
    #[must_use]
    pub fn all() -> Self {
        Self(None)
    }

    /// Exactly `signal`.
    #[must_use]
    pub fn one(signal: SignalId) -> Self {
        Self(Some(Arc::from([signal])))
    }

    /// Exactly `signals`, in any order, duplicates allowed. Sorts a copy.
    #[must_use]
    pub fn many(signals: impl IntoIterator<Item = SignalId>) -> Self {
        let mut set: Vec<_> = signals.into_iter().collect();
        set.sort_unstable();
        set.dedup();
        Self(Some(Arc::from(set)))
    }

    /// The explicit set, or `None` for every signal.
    #[must_use]
    pub fn signals(&self) -> Option<&[SignalId]> {
        self.0.as_deref()
    }

    /// Whether `signal` is covered.
    #[must_use]
    pub fn matches(&self, signal: SignalId) -> bool {
        self.0
            .as_ref()
            .is_none_or(|s| s.binary_search(&signal).is_ok())
    }

    /// Whether any covered signal lies in `min..=max`. This is what spares a
    /// segment its head read when the signal range rules it out.
    #[must_use]
    pub fn may_hold(&self, min: SignalId, max: SignalId) -> bool {
        self.0.as_ref().is_none_or(|s| {
            let first = s.partition_point(|x| *x < min);
            s.get(first).is_some_and(|x| *x <= max)
        })
    }
}

impl Default for SignalFilter {
    /// Every signal.
    fn default() -> Self {
        Self::all()
    }
}

/// One lookup: signals and streams over an inclusive time range.
///
/// The four questions of SPEC §21 are shapes of this: signal → chunks is
/// [`PlanRequest::signals`], time range → chunks is [`PlanRequest::new`] alone,
/// stream → chunks is [`PlanRequest::stream`] over [`PlanRequest::everything`],
/// and nearest is [`crate::Snapshot::nearest_at_or_before`]. Entity → signals is
/// not: ask `norn-metadata`, then pass the signals here.
///
/// Concurrency: `Send + Sync`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlanRequest {
    /// The earliest timestamp, inclusive. Chunks are not clipped: one starting
    /// earlier is returned whole, and the reader drops the samples before it.
    pub from: Timestamp,
    /// The latest timestamp, inclusive. Past `to` yields nothing, nor does
    /// `from > to`.
    pub to: Timestamp,
    /// The signals covered. Default: every signal.
    pub signals: SignalFilter,
    /// The one stream covered. Default: every stream.
    pub stream: Option<StreamId>,
}

impl PlanRequest {
    /// Every signal of every stream in `from..=to`.
    #[must_use]
    pub fn new(from: Timestamp, to: Timestamp) -> Self {
        Self {
            from,
            to,
            signals: SignalFilter::all(),
            stream: None,
        }
    }

    /// All of time.
    #[must_use]
    pub fn everything() -> Self {
        Self::new(Timestamp::MIN, Timestamp::MAX)
    }

    /// Restricted to `signals`.
    #[must_use]
    pub fn signals(mut self, signals: SignalFilter) -> Self {
        self.signals = signals;
        self
    }

    /// Restricted to `stream`.
    #[must_use]
    pub fn stream(mut self, stream: StreamId) -> Self {
        self.stream = Some(stream);
        self
    }
}
