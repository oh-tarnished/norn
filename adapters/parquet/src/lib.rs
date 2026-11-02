//! Parquet and the Arrow IPC file format: columnar interchange (SPEC §12, §44).
//! Neither is Norn's storage; both are for other tools.
//!
//! # Layer contract
//!
//! - **Owns**: the column layout in [`columns`], [`ParquetSink`] and
//!   [`ParquetSource`], [`ArrowSink`] and [`ArrowSource`].
//! - **Must not**: reach the daemon; link C (Snappy is the only codec, pure Rust);
//!   hold more than a bounded row group, or a bounded batch.
//! - **May import**: `norn-adapter`, `norn-protocol`; third-party: `parquet`,
//!   `arrow-*`, `crc32fast`, `prost`.
//! - **Spans**: none.
//!
//! # Format
//!
//! One row per sample, sorted by (signal, time, sequence). [`columns::schema`]
//! lists the columns: `signal`, `stream`, `value_type`, `time`, `sequence`,
//! `severity`, `substatus`, and one nullable column per family of value, of which
//! `value_type` says which is set.
//!
//! The file's key-value metadata holds the catalog (`norn.catalog`) and a CRC-32
//! per row group or batch (`norn.digests`), since neither writer stores a data
//! checksum and SPEC §20 will not accept damage silently. A file with no digests
//! (another tool's, with these columns) is read unchecked; with no catalog, its
//! signals are discovered row by row.

mod arrow_file;
pub mod columns;
mod digest;
mod from_batch;
mod reader;
mod writer;

pub use arrow_file::{ArrowSink, ArrowSource};
pub use reader::ParquetSource;
pub use writer::ParquetSink;

/// The key of the digests in file metadata: `rows:crc32` per group, comma-separated.
pub(crate) const DIGESTS_KEY: &str = "norn.digests";
