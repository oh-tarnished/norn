//! What goes into the write path and what it says back (SPEC §18, §19).

use std::sync::Arc;
use std::time::Duration;

use norn_core::{Sample, StreamId};
use tokio::sync::oneshot;

/// How far a batch has travelled. Each level implies those below it; docs/reference/protocol.md
/// defines each in exact words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs, reason = "docs/reference/protocol.md defines each level")]
pub enum AckLevel {
    Received,
    Buffered,
    Persisted,
    Durable,
}

/// What happened to a batch. Only `Reject` refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    missing_docs,
    reason = "docs/reference/protocol.md defines each outcome"
)]
pub enum Outcome {
    Accept,
    Buffer,
    Throttle,
    Reject,
}

/// Why a batch got its outcome; the protocol's `BackpressureReason` values that
/// the write path itself decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[allow(
    missing_docs,
    reason = "docs/reference/protocol.md defines each reason"
)]
pub enum Reason {
    CapacityAvailable,
    WritePathBehind,
    BufferHighWater,
    BufferFull,
    MemoryExhausted,
    Throttled,
    ShuttingDown,
    StorageUnavailable,
    BatchTooLarge,
    AckLevelUnavailable,
}

/// An admission decision, always with a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Decision {
    /// What happened.
    pub outcome: Outcome,
    /// Why.
    pub reason: Reason,
    /// For `Throttle` and a transient `Reject`, how long to wait.
    pub retry_delay: Option<Duration>,
    /// For logs only; never parse it.
    pub message: String,
}

impl Decision {
    pub(crate) fn admit(outcome: Outcome, reason: Reason, retry_delay: Option<Duration>) -> Self {
        Self {
            outcome,
            reason,
            retry_delay,
            message: String::new(),
        }
    }

    pub(crate) fn reject(reason: Reason, retry: Option<Duration>, message: String) -> Self {
        Self {
            outcome: Outcome::Reject,
            reason,
            retry_delay: retry,
            message,
        }
    }
}

/// One batch, already validated: every sample's signal exists, records into
/// `stream`, and its value has the signal's kind. Moved in; never copied.
#[derive(Debug)]
#[non_exhaustive]
pub struct Batch {
    /// The stream.
    pub stream: StreamId,
    /// The samples, in any order.
    pub samples: Vec<Sample>,
    /// The level the caller wants to hear about.
    pub level: AckLevel,
}

impl Batch {
    /// A batch from its parts.
    #[must_use]
    pub fn new(stream: StreamId, samples: Vec<Sample>, level: AckLevel) -> Self {
        Self {
            stream,
            samples,
            level,
        }
    }
}

/// Why an admitted batch can no longer reach its level. For logs and the
/// response's `error`; the failed stream refuses further batches until restart.
pub type AckFailure = Arc<str>;

/// Resolves once the batch reaches the level it asked for, or fails. Dropping it
/// changes nothing about the batch.
pub type Completion = oneshot::Receiver<Result<AckLevel, AckFailure>>;

/// The answer to [`Ingest::admit`](super::Ingest::admit).
#[derive(Debug)]
#[non_exhaustive]
pub struct Admission {
    /// The decision.
    pub decision: Decision,
    /// Samples of the batch already held, counted and not stored again.
    pub duplicates: u64,
    /// The level reached; `None` when rejected.
    pub reached: Option<AckLevel>,
    /// Present when the requested level is above `reached`.
    pub completion: Option<Completion>,
}

impl Admission {
    pub(crate) fn rejected(decision: Decision) -> Self {
        Self {
            decision,
            duplicates: 0,
            reached: None,
            completion: None,
        }
    }
}
