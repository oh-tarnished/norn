//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! MCAP: one channel per signal, protobuf-encoded, for Foxglove and Lichtblick
//! (SPEC §12, §44, §45).
//!
//! # Layer contract
//!
//! - **Owns**: [`McapSink`] and [`McapSource`], and the committed descriptor of
//!   `norn.type.Sample`, which `just gen` rebuilds and `just gen-check` compares.
//! - **Must not**: reach the daemon; link C (the crate's zstd and lz4 features stay
//!   off, so chunks are uncompressed, and a compressed file is refused by name).
//! - **May import**: `norn-adapter`, `norn-protocol`; third-party: `mcap`, `prost`.
//! - **Spans**: none.
//!
//! # Format
//!
//! Each signal is a channel whose topic is the signal's name, with message
//! encoding `protobuf` and one shared schema: `norn.type.Sample`, encoding
//! `protobuf`, whose data is a `FileDescriptorSet` holding it and its imports.
//! Every message is one `Sample`, carrying the sample in full. Channel metadata
//! gives `stream`, `value_type` and `unit`. A metadata record named `norn.catalog`,
//! key `json`, carries the catalog. Messages are in time order, which is what a
//! player reads.
//!
//! `log_time` and `publish_time` are the sample's time in nanoseconds. MCAP's are
//! unsigned, so a sample before 1970 logs at 0; its message keeps the true time.
//! Channel ids are 16 bits, so a file holds at most 65,535 signals; past that the
//! export fails with [`norn_adapter::AdapterError::Encode`].

mod records;
mod sink;
mod source;

pub use sink::McapSink;
pub use source::McapSource;

/// The schema name, the full protobuf name of the message.
pub const SCHEMA_NAME: &str = "norn.type.Sample";

/// The encoding of the schema, and of messages.
pub(crate) const PROTOBUF: &str = "protobuf";

/// The metadata record holding the catalog, and its one key.
pub(crate) const CATALOG_RECORD: &str = norn_adapter::CATALOG_KEY;
pub(crate) const CATALOG_FIELD: &str = "json";

/// The format's name, as errors give it.
pub(crate) const FORMAT: &str = "mcap";

/// A `google.protobuf.FileDescriptorSet` of `norn.type.Sample` and its imports.
pub const SAMPLE_DESCRIPTOR: &[u8] = include_bytes!("generated/norn_type_sample.binpb");
