//! What the registry keeps about each resource: the core's descriptor plus
//! what only the resource layer needs.
//!
//! Concurrency, for all: owned data, `Send + Sync`. A record handed out is a
//! copy; it does not track later changes.

use std::time::Duration;

use norn_core::{EntityDescriptor, SignalDescriptor, StreamDescriptor, StreamId, Timestamp};

/// One entity. `descriptor.name` is its resource ID.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EntityRecord {
    /// The compact ID, the resource ID as `name`, and the annotations.
    pub descriptor: EntityDescriptor,
    /// A human-readable name; may be empty.
    pub display_name: String,
    /// When it was created.
    pub create_time: Timestamp,
    /// When it last changed.
    pub update_time: Timestamp,
    /// Increases with every change; the etag is made from it.
    pub revision: u64,
}

/// One signal. `descriptor.name` is its resource ID within its entity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignalRecord {
    /// The compact IDs, value kind and annotations.
    pub descriptor: SignalDescriptor,
    /// The stream it records into; immutable.
    pub stream: StreamId,
    /// A human-readable name; may be empty.
    pub display_name: String,
    /// The unit, free text; never interpreted.
    pub unit: String,
    /// When it was created.
    pub create_time: Timestamp,
    /// When its metadata last changed.
    pub update_time: Timestamp,
    /// Increases with every change; the etag is made from it.
    pub revision: u64,
}

/// How long a stream keeps history (SPEC §43). Recorded here and published;
/// nothing applies it until a retention pass exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Retention {
    /// Keep everything.
    #[default]
    Forever,
    /// Remove chunks whose newest sample is older than this.
    MaxAge(Duration),
    /// Remove the oldest chunks while the stream is larger than this.
    MaxBytes(u64),
}

/// One stream. `descriptor.name` is its resource ID.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StreamRecord {
    /// The compact ID, the resource ID as `name`, and the annotations.
    pub descriptor: StreamDescriptor,
    /// A human-readable name; may be empty.
    pub display_name: String,
    /// The retention policy.
    pub retention: Retention,
    /// When it was created.
    pub create_time: Timestamp,
    /// When its settings last changed.
    pub update_time: Timestamp,
    /// Increases with every change; the etag is made from it.
    pub revision: u64,
}
