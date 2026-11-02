//! Norn's native on-disk format (SPEC §12, §13, §20, §40, §42, §43).
//!
//! # Layer contract
//!
//! - **Owns** the byte layout of segment files; writing, reading and
//!   recovering them; the format side of compaction; and retention listings.
//!   The hierarchy is Database → Stream ([`stream_dir`]) → Segment
//!   ([`SegmentWriter`], [`SegmentReader`]) → Chunk ([`Chunk`]) → Block (a
//!   `norn-codec` column block) → Sample. docs/reference/storage.md has the byte tables.
//! - **Must not** know any domain, read a clock (segment spans are measured
//!   on sample timestamps), spawn a thread, decide acknowledgement levels
//!   (it offers `flush` and `sync`; the write path maps them to SPEC §19) or
//!   delete data on its own initiative (SPEC §43).
//! - **May import** `norn-core` and `norn-codec` only. No third-party crate.
//! - **Spans**: none yet; recovery findings are returned as typed values.
//!
//! # Concurrency
//!
//! No global state. Writers and readers take `&mut self` and are `Send`.
//! One writer per stream directory, enforced by [`StreamLock`]; any number of
//! readers, each with its own [`SegmentReader`]. A sealed segment never changes.
//!
//! # Failure
//!
//! Nothing panics on malformed input. Damage is a typed
//! [`StorageError::Corrupt`] on read and a [`RecoveryEvent`] in recovery, and
//! bytes are never discarded without first being quarantined.
//!
//! # Memory
//!
//! Every bound is in [`StorageOptions`], derived from `norn_core::Config`.
//! Reads use plain `seek` + `read`, not `mmap`, because this crate has no
//! `unsafe`; mapping is a later, measured option.

#![forbid(unsafe_code)]

mod chunk;
mod compact;
mod corruption;
mod crc32c;
mod error;
pub mod format;
mod fsio;
mod iter;
mod layout;
mod lock;
mod options;
mod platform;
mod recovery;
mod retention;
mod segment;
mod stream;

pub use chunk::{Chunk, Column, EncodedChunk};
pub use compact::{CompactionOutcome, compact};
pub use crc32c::{crc32c, extend as crc32c_extend};
pub use error::{Bound, Corruption, CorruptionKind, Result, StorageError, VersionField};
pub use format::frame::ChunkMeta;
pub use format::{FORMAT_VERSION, SCHEMA_VERSION, SegmentHeader};
pub use fsio::atomic_write;
pub use iter::SampleIter;
pub use layout::{SegmentEntry, list_segments, live_segments, stream_dir};
pub use lock::{LOCK_FILE, StreamLock};
pub use options::StorageOptions;
pub use platform::sync_dir;
pub use recovery::{
    Action, QUARANTINE_DIR, RecoveryEvent, RecoveryMode, RecoveryReport, SegmentRecovery,
    recover_segment, recover_stream,
};
pub use retention::{segments_before, segments_over_size};
pub use segment::{
    ChunkFile, SEGMENT_EXT, SegmentName, SegmentReader, SegmentSummary, SegmentWriter,
};
pub use stream::{Appended, StreamWriter};
