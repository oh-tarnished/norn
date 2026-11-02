//! One timestamped observation.

use crate::id::SignalId;
use crate::quality::Quality;
use crate::time::{Sequence, Timestamp};
use crate::value::Value;

/// A timestamped observation of one signal (SPEC §10): conceptually the row
/// `timestamp, signal, value, quality, sequence` that Norn stores.
///
/// The entity is not repeated per sample; it is reachable through the
/// signal's descriptor, which keeps the hot row small.
///
/// The core does not check that `value` matches the signal's declared
/// [`crate::ValueKind`]: it holds no registry of signals. The ingest path
/// that does hold one checks it.
///
/// Concurrency: owned data; `Send + Sync`.
/// Performance: fields are inline except a `String`/`Bytes` payload.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Sample {
    /// When the observation was made, as reported by its source.
    pub timestamp: Timestamp,
    /// Which signal was observed.
    pub signal: SignalId,
    /// What was observed.
    pub value: Value,
    /// How far the observation can be trusted.
    pub quality: Quality,
    /// Its position in its stream; orders samples with equal timestamps.
    pub sequence: Sequence,
}

impl Sample {
    /// A sample from its parts.
    #[must_use]
    pub fn new(
        timestamp: Timestamp,
        signal: SignalId,
        value: Value,
        quality: Quality,
        sequence: Sequence,
    ) -> Self {
        Self {
            timestamp,
            signal,
            value,
            quality,
            sequence,
        }
    }
}
