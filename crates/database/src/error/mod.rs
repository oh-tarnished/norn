//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What can go wrong with a database as a whole. Callers match on variants.

mod display;

use std::path::PathBuf;

use norn_metadata::MetadataError;
use norn_storage::StorageError;

/// Which unfinished job a database records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unfinished {
    /// `nornctl migrate` stopped before its last step: run it again.
    Migration,
    /// `nornctl restore` stopped before its last step: run it again.
    Restore,
}

/// Why a database-level operation failed.
///
/// `#[non_exhaustive]`: match with a wildcard arm. `Send + Sync`.
#[derive(Debug)]
#[non_exhaustive]
pub enum DatabaseError {
    /// An operating-system call failed.
    Io {
        /// What was being done.
        op: &'static str,
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A segment could not be read or assessed.
    Storage(StorageError),
    /// The metadata log could not be read.
    Metadata(MetadataError),
    /// A daemon, or another tool, holds the database's lock.
    Locked {
        /// The lock file.
        path: PathBuf,
    },
    /// A record this crate writes (`FORMAT`, `MIGRATION`, `RESTORING`, a backup
    /// `MANIFEST`) failed its checksum or does not parse.
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What was wrong.
        what: String,
    },
    /// The database was written by a newer Norn; nothing was changed.
    TooNew {
        /// Its store version.
        found: u16,
        /// The newest this build reads.
        supported: u16,
    },
    /// The database is older than this build reads in place, and can be
    /// migrated: `nornctl migrate --data-dir D --to <current>`.
    NeedsMigration {
        /// The database.
        root: PathBuf,
        /// Its store version.
        found: u16,
        /// This build's store version.
        current: u16,
    },
    /// The database is older than any version this build can migrate from.
    TooOld {
        /// Its store version.
        found: u16,
        /// The oldest this build migrates.
        oldest: u16,
    },
    /// A migration or restore was interrupted; repeating the command finishes it.
    Unfinished {
        /// The database.
        root: PathBuf,
        /// Which.
        job: Unfinished,
    },
    /// No chain of migrations leads from `from` to `to`.
    NoMigration {
        /// The store version found.
        from: u16,
        /// The one asked for.
        to: u16,
    },
    /// A destination that must be empty is not.
    NotEmpty {
        /// The directory.
        path: PathBuf,
    },
    /// A file failed its recorded length or CRC-32C.
    Mismatch {
        /// The file.
        path: PathBuf,
        /// What was recorded.
        expected: (u64, u32),
        /// What was found.
        found: (u64, u32),
    },
    /// Verification found damage; the count is of damaged regions.
    Damaged {
        /// The database or backup.
        root: PathBuf,
        /// Regions that failed a check.
        regions: u64,
    },
    /// A test's stop point was reached: the job stopped as a crash would.
    Interrupted,
}

impl DatabaseError {
    pub(crate) fn io(op: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            op,
            path: path.into(),
            source,
        }
    }

    pub(crate) fn corrupt(path: impl Into<PathBuf>, what: impl Into<String>) -> Self {
        Self::Corrupt {
            path: path.into(),
            what: what.into(),
        }
    }
}

impl From<StorageError> for DatabaseError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

impl From<MetadataError> for DatabaseError {
    fn from(e: MetadataError) -> Self {
        Self::Metadata(e)
    }
}

/// Shorthand used throughout the crate.
pub type Result<T> = std::result::Result<T, DatabaseError>;
