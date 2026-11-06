//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl migrate`: moving a stopped database to a newer store version.
//!
//! Offline: it holds the daemon's lock. Crash-safe and resumable: each step writes
//! new files beside the old, fsyncs them, commits by replacing `MIGRATION`, renames
//! them into place keeping the old ones, moves `FORMAT`, reads the result through,
//! and only then removes the old files, unless asked to keep them. The daemon refuses
//! the database while `MIGRATION` exists. Running the command again continues from
//! the record, whatever killed the last run.
//!
//! Today [`MIGRATIONS`] is empty: store version 1 is the only one, and the one
//! migration is the identity on it, which records the version in a database written
//! by 1.0.0-rc.1. `tests/migrate.rs` drives a test-only step down the whole path.

mod files;
mod record;
mod step;

use std::path::Path;

use self::files::database_files;
use self::record::{Phase, Record};
use crate::error::{DatabaseError, Result};
use crate::inspect::inspect;
use crate::lock::DatabaseLock;
use crate::marker;
use crate::version::{CURRENT_VERSION, LEGACY_VERSION};

/// One step up the store versions.
///
/// Concurrency: plain functions and data, shared freely.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// The store version it starts from.
    pub from: u16,
    /// The store version it reaches: `from + 1`, always.
    pub to: u16,
    /// Whether it might change a file, by its path inside the database.
    pub applies: fn(&str) -> bool,
    /// Writes the new form of the file at the first path to the second, and says
    /// whether it did; false leaves the file as it is. Must not touch the original.
    /// The engine fsyncs the result. An `Err` is a message for people.
    pub rewrite: fn(&Path, &Path) -> std::result::Result<bool, String>,
}

/// The steps this build knows, oldest first.
pub const MIGRATIONS: &[Migration] = &[];

/// What to migrate to, and how.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrateOptions {
    /// The store version wanted.
    pub to: u16,
    /// Say what would happen; change nothing.
    pub dry_run: bool,
    /// Keep each replaced file beside the new one, as `<name>.v<from>-old`.
    pub keep_old: bool,
}

/// What a migration did, or under `dry_run` would do.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationReport {
    /// The store version found.
    pub from: u16,
    /// The store version it now has, or under `dry_run` would.
    pub to: u16,
    /// Steps run, or that would run.
    pub steps: u64,
    /// Files rewritten; under `dry_run`, files the steps would look at.
    pub files: u64,
    /// Whether it continued an interrupted run.
    pub resumed: bool,
    /// Segments read through at the end. Zero under `dry_run`.
    pub verified_segments: u64,
    /// Damaged regions found at the end, which it did not add to: a step that does
    /// fails instead. Zero under `dry_run`.
    pub damaged_regions: u64,
}

/// Migrates the database at `root` with this build's steps.
///
/// # Errors
/// As [`migrate_with`].
pub fn migrate(root: &Path, options: MigrateOptions) -> Result<MigrationReport> {
    migrate_with(root, options, MIGRATIONS, &|_| false)
}

/// Migrates the database at `root` along `chain`, stopping as a crash would wherever
/// `stop` says, for a test. Blocks on the disk.
///
/// # Errors
/// [`DatabaseError::Locked`] while a daemon runs; [`DatabaseError::TooNew`];
/// [`DatabaseError::NoMigration`] when `chain` does not reach `to`, a lower version
/// included; [`DatabaseError::Damaged`] when the result has more damage than the
/// start, with the old files kept; [`DatabaseError::Interrupted`]; those of the files.
pub fn migrate_with(
    root: &Path,
    options: MigrateOptions,
    chain: &[Migration],
    stop: step::Stop<'_>,
) -> Result<MigrationReport> {
    let lock = DatabaseLock::acquire(root, false)?;
    let pending = Record::read(root)?;
    let start = marker::read(root)?.unwrap_or(LEGACY_VERSION);
    let from = pending.as_ref().map_or(start, |r| r.from);
    if from > CURRENT_VERSION.max(options.to) {
        return Err(DatabaseError::TooNew {
            found: from,
            supported: CURRENT_VERSION,
        });
    }
    if pending.as_ref().is_some_and(|r| r.to > options.to) {
        // An unfinished step goes where it was going: a lower target cannot undo it.
        return Err(DatabaseError::NoMigration {
            from,
            to: options.to,
        });
    }
    let steps = plan(chain, from, options.to)?;
    let mut report = MigrationReport {
        from,
        to: options.to,
        steps: steps.len() as u64,
        resumed: pending.is_some(),
        ..MigrationReport::default()
    };
    if options.dry_run {
        let all = database_files(root)?;
        let relevant = |f: &&String| steps.iter().any(|s| (s.applies)(f));
        report.files = all.iter().filter(relevant).count() as u64;
        return Ok(report);
    }
    let mut pending = pending;
    for migration in &steps {
        let record = match pending.take() {
            Some(record) => record,
            None => {
                let record = Record {
                    from: migration.from,
                    to: migration.to,
                    phase: Phase::Prepare,
                    // A build may not be able to read the old version at all, or a
                    // step would not be needed. Then the result must have no damage.
                    damage: inspect(root).map_or(0, |found| found.damaged_regions),
                    files: Vec::new(),
                };
                record.write(root)?;
                record
            }
        };
        report.files += step::run(root, migration, record, options.keep_old, stop)? as u64;
    }
    // The identity at the top: a database written before `FORMAT` existed gets one.
    if marker::read(root)?.is_none() {
        marker::write(root, options.to)?;
    }
    let found = inspect(root)?;
    report.verified_segments = found.verified_segments;
    report.damaged_regions = found.damaged_regions;
    drop(lock);
    Ok(report)
}

/// The steps leading from `from` to `to`. `from == to` needs none.
fn plan(chain: &[Migration], from: u16, to: u16) -> Result<Vec<Migration>> {
    let mut steps = Vec::new();
    let mut at = from;
    while at < to {
        let next = chain.iter().find(|m| m.from == at);
        let next = next.ok_or(DatabaseError::NoMigration { from, to })?;
        steps.push(*next);
        at = next.to;
    }
    if at == to {
        Ok(steps)
    } else {
        Err(DatabaseError::NoMigration { from, to })
    }
}
