//! Norn's domain-independent temporal types.
//!
//! # Layer contract
//!
//! - **Owns**: the vocabulary every other layer speaks: identifiers
//!   ([`EntityId`], [`SignalId`], [`StreamId`]), time ([`Timestamp`],
//!   [`Sequence`]), observations ([`Value`], [`Quality`], [`Sample`]),
//!   descriptors with opaque [`Metadata`], the typed [`Error`], and the
//!   validated [`Config`] of memory budgets and limits.
//! - **Must not**: perform I/O, read a clock, spawn a thread, or name any
//!   domain. BACnet, Modbus, AHU or building appear only as strings in
//!   [`Metadata`], put there by adapters (SPEC §4). `xtask purity` enforces the
//!   first three.
//! - **May import**: `std` only. No runtime dependencies; `proptest` is a
//!   dev-dependency.
//! - **Spans**: none; the core has no operations worth tracing.
//!
//! Concurrency: every type here is plain data, `Send + Sync`, with no
//! interior mutability. Nothing panics: fallible operations return
//! [`Result`].

mod config;
mod descriptor;
mod error;
mod id;
mod metadata;
mod name;
mod quality;
mod sample;
mod time;
mod value;

pub use config::{Config, ConfigBuilder, Limits, MaintenanceConfig, MemoryBudgets, StorageConfig};
pub use descriptor::{EntityDescriptor, SignalDescriptor, StreamDescriptor};
pub use error::{ConfigError, ConfigField, Error, Limit, NameError, Result};
pub use id::{EntityId, SignalId, StreamId};
pub use metadata::{Metadata, MetadataLimits};
pub use name::Name;
pub use quality::{Quality, Severity, Substatus};
pub use sample::Sample;
pub use time::{Sequence, Timestamp};
pub use value::{Value, ValueKind};
