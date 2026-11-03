//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The Norn adapter contract: one way for external data to enter and leave Norn.
//!
//! # Layer contract
//!
//! - **Owns**: the [`SampleSink`] and [`SampleSource`] traits a format implements,
//!   the portable [`Catalog`] of streams, entities and signals, the textual names
//!   of value types and quality, and the two drivers: [`export()`] and [`import()`].
//! - **Must not**: reach the daemon any way but the public gRPC API (SPEC §29,
//!   §47); know any format or any domain; hold more than one bounded batch of
//!   samples, whatever the size of the history (SPEC §44, §45).
//! - **May import**: `norn-protocol`, whose message types are this crate's
//!   vocabulary for Entity, Signal, Sample and Metadata (annotations), and
//!   `norn-core`, `norn-transport`, `norn-ipc`; third-party: `tonic`, `tokio`,
//!   `prost`, `prost-types`, `serde`, `serde_json`.
//! - **Spans**: none yet.
//!
//! A third-party adapter (one maintained outside this repository) needs only this crate or
//! the protocol: it implements a trait, or it calls `AppendSamples` itself.
//!
//! The drivers are `async` and call the traits inline, so reading or writing a
//! file blocks the task: on a slow disk that is the backpressure, and it holds no
//! queue. Run them on their own task or runtime if the thread has other work.

mod catalog;
mod catalog_json;
mod discovery;
mod error;
mod export;
mod import;
mod names;
mod traits;
mod wire;

pub mod time;

pub use catalog::{Catalog, entity_of, entity_resource};
pub use discovery::Discovery;
pub use error::AdapterError;
pub use export::{ExportOptions, ExportReport, Selection, export};
pub use import::{ImportOptions, ImportReport, import};
pub use names::{
    parse_severity, parse_substatus, parse_value_type, severity_name, substatus_name,
    value_type_name, value_type_of,
};
pub use traits::{SampleSink, SampleSource, SourceBatch};

/// A file's or a source's signals past this many are refused (SPEC §41), since
/// a catalog is the one thing the adapters hold per signal.
pub const DEFAULT_MAX_SIGNALS: usize = 1_000_000;

/// The file key holding the [`Catalog`] in Parquet, Arrow and MCAP.
pub const CATALOG_KEY: &str = "norn.catalog";
