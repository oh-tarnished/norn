//! What a query needs from samples not yet in the index.

use std::fmt;
use std::ops::ControlFlow;

use norn_core::{Sample, SignalId, StreamId, Timestamp};
use norn_storage::SegmentName;

/// The samples a write path holds that may not be in the index yet.
///
/// A trait because this crate may not import `norn-runtime`, which implements it
/// over its per-signal buffers. Freshness rests on this contract:
///
/// - A sample is visible here from before its `PERSISTED` acknowledgement until
///   **after** it is visible in the index. There may be an overlap and never a
///   gap, so a reader that asks here first and takes its index snapshot second
///   misses nothing, and removes the overlap by comparing samples.
///
/// Concurrency: shared by every query, and called alongside ingest. `visit` may
/// run under a lock of the implementor's, so it never waits. Memory: each call
/// copies what it visits and holds nothing past the call.
pub trait TailSource: Send + Sync + fmt::Debug {
    /// Calls `visit` for each buffered sample of `signal` in `from..=to`, in no
    /// particular order, until it says stop.
    fn scan(
        &self,
        stream: StreamId,
        signal: SignalId,
        from: Timestamp,
        to: Timestamp,
        visit: &mut dyn FnMut(Sample) -> ControlFlow<()>,
    );

    /// The latest buffered timestamp of `signal` at or before `at`.
    fn newest_at_or_before(
        &self,
        stream: StreamId,
        signal: SignalId,
        at: Timestamp,
    ) -> Option<Timestamp>;

    /// A read found the chunk at `offset` of `segment`, claiming `samples`
    /// samples, damaged. The owner of the stream's totals may then count them
    /// out at once. Called on the query's thread, so keep it short. Default:
    /// nothing, since the query reports the damage to its caller regardless.
    fn damaged(&self, stream: StreamId, segment: SegmentName, offset: u64, samples: u32) {
        let _ = (stream, segment, offset, samples);
    }
}

/// No tail: what a database no write path is running over looks like.
///
/// Concurrency: stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTail;

impl TailSource for NoTail {
    fn scan(
        &self,
        _: StreamId,
        _: SignalId,
        _: Timestamp,
        _: Timestamp,
        _: &mut dyn FnMut(Sample) -> ControlFlow<()>,
    ) {
    }

    fn newest_at_or_before(&self, _: StreamId, _: SignalId, _: Timestamp) -> Option<Timestamp> {
        None
    }
}
