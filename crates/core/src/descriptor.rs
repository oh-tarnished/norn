//! Descriptions of entities, signals and streams.
//!
//! A descriptor is the slowly-changing "what is this" record; samples are the
//! fast-changing "what did it say". Each carries an opaque [`Metadata`] map
//! for everything the core must not know (protocol, units, vendor, location).
//!
//! Concurrency, for all three: owned data, `Send + Sync`; mutation of the
//! metadata needs `&mut`. Fields are public: a descriptor's invariants are
//! held by its field types ([`Name`] and [`Metadata`] validate themselves).

use crate::id::{EntityId, SignalId, StreamId};
use crate::metadata::Metadata;
use crate::name::Name;
use crate::value::ValueKind;

/// A thing that exists: a building, a floor, a motor, a vehicle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EntityDescriptor {
    /// The entity's identifier.
    pub id: EntityId,
    /// The entity's name, e.g. `AHU-01`.
    pub name: Name,
    /// Adapter-supplied context the core never interprets.
    pub metadata: Metadata,
}

impl EntityDescriptor {
    /// An entity with empty metadata.
    #[must_use]
    pub fn new(id: EntityId, name: Name) -> Self {
        Self {
            id,
            name,
            metadata: Metadata::default(),
        }
    }
}

/// A measurable or observable property of an entity: a temperature, a
/// position, an operating state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignalDescriptor {
    /// The signal's identifier.
    pub id: SignalId,
    /// The entity this signal belongs to.
    pub entity: EntityId,
    /// The signal's name, e.g. `SupplyAirTemperature`.
    pub name: Name,
    /// The kind every sample of this signal carries; storage chooses its
    /// encoding from it (SPEC §14).
    pub kind: ValueKind,
    /// Adapter-supplied context: units, protocol addressing, enum labels.
    pub metadata: Metadata,
}

impl SignalDescriptor {
    /// A signal with empty metadata.
    #[must_use]
    pub fn new(id: SignalId, entity: EntityId, name: Name, kind: ValueKind) -> Self {
        Self {
            id,
            entity,
            name,
            kind,
            metadata: Metadata::default(),
        }
    }
}

/// A logical sequence of temporal observations, such as one site's telemetry
/// or one import.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StreamDescriptor {
    /// The stream's identifier.
    pub id: StreamId,
    /// The stream's name.
    pub name: Name,
    /// Adapter-supplied context: source, import file, collection policy.
    pub metadata: Metadata,
}

impl StreamDescriptor {
    /// A stream with empty metadata.
    #[must_use]
    pub fn new(id: StreamId, name: Name) -> Self {
        Self {
            id,
            name,
            metadata: Metadata::default(),
        }
    }
}
