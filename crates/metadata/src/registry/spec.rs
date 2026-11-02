//! What a caller supplies to create or change a resource.
//!
//! Plain owned data; `#[non_exhaustive]` so a new field is not a breaking
//! change, which is why each has a constructor.

use norn_core::{Metadata, ValueKind};

#[allow(unused_imports, reason = "for the doc links")]
use super::Registry;
use crate::record::Retention;

/// A new entity's settable fields. `Default` is empty.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct EntitySpec {
    /// A human-readable name.
    pub display_name: String,
    /// Annotations, built with [`Registry::metadata_limits`].
    pub annotations: Metadata,
}

impl EntitySpec {
    /// A spec from its parts.
    #[must_use]
    pub fn new(display_name: String, annotations: Metadata) -> Self {
        Self {
            display_name,
            annotations,
        }
    }
}

/// Which fields of an entity to change: `None` leaves one alone.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct EntityPatch {
    /// A new human-readable name.
    pub display_name: Option<String>,
    /// New annotations, replacing all of them.
    pub annotations: Option<Metadata>,
}

impl EntityPatch {
    /// A patch from its parts.
    #[must_use]
    pub fn new(display_name: Option<String>, annotations: Option<Metadata>) -> Self {
        Self {
            display_name,
            annotations,
        }
    }
}

/// A new stream's settable fields. `Default` keeps history forever.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct StreamSpec {
    /// A human-readable name.
    pub display_name: String,
    /// The retention policy.
    pub retention: Retention,
    /// Annotations, built with [`Registry::metadata_limits`].
    pub annotations: Metadata,
}

impl StreamSpec {
    /// A spec from its parts.
    #[must_use]
    pub fn new(display_name: String, retention: Retention, annotations: Metadata) -> Self {
        Self {
            display_name,
            retention,
            annotations,
        }
    }
}

/// Which fields of a stream to change: `None` leaves one alone.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct StreamPatch {
    /// A new human-readable name.
    pub display_name: Option<String>,
    /// A new retention policy.
    pub retention: Option<Retention>,
    /// New annotations, replacing all of them.
    pub annotations: Option<Metadata>,
}

impl StreamPatch {
    /// A patch from its parts.
    #[must_use]
    pub fn new(
        display_name: Option<String>,
        retention: Option<Retention>,
        annotations: Option<Metadata>,
    ) -> Self {
        Self {
            display_name,
            retention,
            annotations,
        }
    }
}

/// A new signal's fields; `kind` and `stream` are immutable afterwards.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SignalSpec {
    /// The value kind of every sample.
    pub kind: ValueKind,
    /// The resource ID of the stream it records into.
    pub stream: String,
    /// A human-readable name.
    pub display_name: String,
    /// The unit, free text.
    pub unit: String,
    /// Annotations, built with [`Registry::metadata_limits`].
    pub annotations: Metadata,
}

impl SignalSpec {
    /// A spec for `kind` in `stream`, with the optional fields empty.
    #[must_use]
    pub fn new(kind: ValueKind, stream: String) -> Self {
        Self {
            kind,
            stream,
            display_name: String::new(),
            unit: String::new(),
            annotations: Metadata::default(),
        }
    }

    /// Sets the optional fields.
    #[must_use]
    pub fn describe(mut self, display_name: String, unit: String, annotations: Metadata) -> Self {
        self.display_name = display_name;
        self.unit = unit;
        self.annotations = annotations;
        self
    }
}

/// Which fields of a signal to change: `None` leaves one alone. Value kind and
/// stream are absent: both are immutable.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SignalPatch {
    /// A new human-readable name.
    pub display_name: Option<String>,
    /// A new unit.
    pub unit: Option<String>,
    /// New annotations, replacing all of them.
    pub annotations: Option<Metadata>,
}

impl SignalPatch {
    /// A patch from its parts.
    #[must_use]
    pub fn new(
        display_name: Option<String>,
        unit: Option<String>,
        annotations: Option<Metadata>,
    ) -> Self {
        Self {
            display_name,
            unit,
            annotations,
        }
    }
}
