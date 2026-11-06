//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The store version, and what a build does with a database of another one.
//!
//! docs/reference/compatibility.md is the policy. The store version moves only
//! when some file must be rewritten before this build can serve the database.
//! A change a build can tell apart file by file does not move it: a segment
//! written as format 1.1, with its encrypted flag, is one such, because every
//! segment says its own version and a reader too old for it refuses that file.

use std::path::{Path, PathBuf};

use crate::error::{DatabaseError, Result, Unfinished};
use crate::marker;

/// The store version this build writes.
pub const CURRENT_VERSION: u16 = 1;
/// The oldest store version this build serves without a migration.
pub const OLDEST_READ: u16 = 1;
/// The oldest store version this build can migrate from.
pub const OLDEST_MIGRATED: u16 = 1;
/// What a database with no `FORMAT` file is: 1.0.0-rc.1 and earlier wrote none.
pub const LEGACY_VERSION: u16 = 1;

/// Written while `nornctl migrate` changes a database; the daemon refuses it.
pub const MIGRATION_FILE: &str = "MIGRATION";

/// A database's store version: its marker's, or the legacy version.
///
/// # Errors
/// As reading `FORMAT` fails.
pub fn store_version(root: &Path) -> Result<u16> {
    Ok(marker::read(root)?.unwrap_or(LEGACY_VERSION))
}

/// Refuses a database this build must not open, before anything in it changes.
/// A directory that does not exist yet, or holds nothing, passes as a new one.
///
/// # Errors
/// [`DatabaseError::Unfinished`] for an interrupted restore or migration;
/// [`DatabaseError::TooNew`], [`DatabaseError::NeedsMigration`] or
/// [`DatabaseError::TooOld`] by version; [`DatabaseError::Corrupt`] for a
/// damaged marker.
pub fn check_openable(root: &Path) -> Result<()> {
    let unfinished = |job| DatabaseError::Unfinished {
        root: PathBuf::from(root),
        job,
    };
    if root.join(marker::RESTORING_FILE).exists() {
        return Err(unfinished(Unfinished::Restore));
    }
    if root.join(MIGRATION_FILE).exists() {
        return Err(unfinished(Unfinished::Migration));
    }
    judge(root, store_version(root)?, OLDEST_READ, OLDEST_MIGRATED)
}

/// What a build whose window is `oldest_read` and `oldest_migrated` does with a
/// database of store version `found`. Pure, so a test can try a wider window than
/// today's, which has only one version in it.
///
/// # Errors
/// As [`check_openable`].
pub fn judge(root: &Path, found: u16, oldest_read: u16, oldest_migrated: u16) -> Result<()> {
    if found > CURRENT_VERSION {
        return Err(DatabaseError::TooNew {
            found,
            supported: CURRENT_VERSION,
        });
    }
    if found >= oldest_read {
        return Ok(());
    }
    if found < oldest_migrated {
        return Err(DatabaseError::TooOld {
            found,
            oldest: oldest_migrated,
        });
    }
    Err(DatabaseError::NeedsMigration {
        root: PathBuf::from(root),
        found,
        current: CURRENT_VERSION,
    })
}

/// Records this build's store version in a database that has no marker yet: a
/// new one, or one written by 1.0.0-rc.1. Call with the database's lock held,
/// after [`check_openable`]. A marker already there is left alone.
///
/// # Errors
/// As reading and writing `FORMAT` fail.
pub fn stamp(root: &Path) -> Result<()> {
    if marker::read(root)?.is_none() {
        marker::write(root, CURRENT_VERSION)?;
    }
    Ok(())
}
