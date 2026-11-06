//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The storage layer's typed errors.
//!
//! Callers match on variants, never on messages. Corruption is its own
//! variant carrying a [`Corruption`] (file, byte range, what was wrong), the
//! same value the recovery report records, so a read error and a recovery
//! event about one torn chunk describe it identically (SPEC §20).

use std::fmt;
use std::path::PathBuf;

use norn_codec::CodecError;

pub use crate::corruption::{Corruption, CorruptionKind};

/// A bounded quantity that a read or write would exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bound {
    /// Bytes in one chunk frame ([`crate::StorageOptions::max_chunk_bytes`]).
    ChunkBytes,
    /// Samples in one chunk ([`crate::StorageOptions::max_chunk_samples`]).
    ChunkSamples,
    /// Chunks in one segment ([`crate::StorageOptions::max_chunks_per_segment`]).
    SegmentChunks,
}

/// Which version field of a segment header a build cannot read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersionField {
    /// The container layout ([`crate::FORMAT_VERSION`]).
    Format,
    /// The chunk column schema ([`crate::SCHEMA_VERSION`]).
    Schema,
    /// The codec block layout ([`norn_codec::CODEC_VERSION`]).
    Codec,
}

/// Why a storage operation failed.
///
/// `#[non_exhaustive]`: match with a wildcard arm. Not `Clone` because it
/// may carry a [`std::io::Error`]. `Send + Sync`.
#[derive(Debug)]
#[non_exhaustive]
pub enum StorageError {
    /// An operating-system call failed.
    Io {
        /// What was being done (`"open"`, `"fsync"`, ...).
        op: &'static str,
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Stored bytes failed a checksum or a structural check.
    Corrupt(Corruption),
    /// The file was written by a newer Norn; refused rather than misread
    /// (SPEC §40).
    UnsupportedVersion {
        /// The segment file.
        path: PathBuf,
        /// Which field.
        field: VersionField,
        /// The stored value.
        found: u16,
        /// The highest value this build reads.
        supported: u16,
    },
    /// A column block inside a checksum-valid chunk failed to decode, or a
    /// column failed to encode.
    Codec {
        /// The segment file (empty when encoding).
        path: PathBuf,
        /// Byte offset of the chunk frame.
        offset: u64,
        /// The codec's own error, with its offset inside the block.
        source: CodecError,
    },
    /// A configured bound would be exceeded; checked before allocating.
    TooLarge {
        /// Which bound.
        bound: Bound,
        /// The size found or requested.
        len: u64,
        /// The bound.
        max: u64,
    },
    /// Samples handed to a chunk do not form one: mixed signals, mixed
    /// value kinds, or none at all.
    InvalidChunk(&'static str),
    /// A file name in a stream directory is not one Norn writes.
    BadName(PathBuf),
    /// The segment was sealed, or its writer failed; it accepts no more
    /// chunks.
    Sealed,
    /// Compaction's preconditions do not hold: no inputs, an unsealed
    /// input, inputs of different streams, or inputs that leave out a live
    /// segment inside their sequence range. Nothing was changed.
    CompactionRefused(&'static str),
    /// A [`crate::CompactionObserver`] stopped a compaction before its commit.
    /// Nothing was changed.
    CompactionCancelled,
    /// Encrypted data could not be opened or written: a wrong key, or a store
    /// and a configuration that disagree. Nothing was changed.
    Encryption(crate::crypt::EncryptionError),
    /// Another writer or repairing recovery holds the stream (SPEC §20): one
    /// writer per stream. It may be this process or another; the lock file
    /// names the holder's process ID, for a human.
    Locked {
        /// The lock file.
        path: PathBuf,
    },
}

impl StorageError {
    /// An [`StorageError::Io`] for `op` on `path`.
    pub(crate) fn io(op: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            op,
            path: path.into(),
            source,
        }
    }

    /// A [`StorageError::Corrupt`] for a region of `path`.
    pub(crate) fn corrupt(
        path: &std::path::Path,
        offset: u64,
        len: u64,
        kind: CorruptionKind,
    ) -> Self {
        Self::Corrupt(Corruption {
            path: path.to_path_buf(),
            offset,
            len,
            kind,
        })
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { op, path, source } => write!(f, "{op} {}: {source}", path.display()),
            Self::Encryption(e) => e.fmt(f),
            Self::Corrupt(c) => write!(
                f,
                "{}: {:?} at bytes {}..+{}",
                c.path.display(),
                c.kind,
                c.offset,
                c.len
            ),
            Self::UnsupportedVersion {
                path,
                field,
                found,
                supported,
            } => write!(
                f,
                "{}: {field:?} version {found} is newer than {supported}",
                path.display()
            ),
            Self::Codec {
                path,
                offset,
                source,
            } => {
                write!(f, "{}: chunk at {offset}: {source}", path.display())
            }
            Self::TooLarge { bound, len, max } => write!(f, "{bound:?}: {len} exceeds {max}"),
            Self::InvalidChunk(why) => write!(f, "invalid chunk: {why}"),
            Self::BadName(p) => write!(f, "not a Norn file name: {}", p.display()),
            Self::Sealed => write!(f, "segment is sealed"),
            Self::CompactionRefused(why) => write!(f, "compaction refused: {why}"),
            Self::CompactionCancelled => f.write_str("compaction cancelled"),
            Self::Locked { path } => write!(f, "another writer holds {}", path.display()),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Codec { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Shorthand used throughout the crate.
pub type Result<T> = std::result::Result<T, StorageError>;
