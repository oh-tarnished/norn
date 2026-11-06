//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A Norn database as a whole: its store version, its backups, restoring one,
//! and migrating it (docs/reference/compatibility.md).
//!
//! # Layer contract
//!
//! - **Owns**: `<database>/FORMAT`, the backup directory layout and `MANIFEST`,
//!   the offline restore, and the migration framework.
//! - **Must not**: parse a chunk or a key. Backups and restores move segments as
//!   opaque bytes, so an encrypted segment needs no key. Nor may it run a daemon.
//! - **May import**: `norn-core`, `norn-storage`, `norn-metadata`. No third-party
//!   crate.
//! - **Spans**: none; every outcome is a typed value or a typed error.
//!
//! # Concurrency
//!
//! Offline jobs hold [`DatabaseLock`], the file the daemon's registry holds, so
//! they and a daemon exclude each other. Blocking throughout: no thread is spawned.
//!
//! # Failure
//!
//! Every job is restartable: run it again after a crash and it finishes.

#![forbid(unsafe_code)]

mod backup;
mod error;
mod inspect;
mod lock;
mod marker;
mod migrate;
mod version;

pub use backup::{
    BackupCheck, BackupSummary, BackupWriter, COPY_BUFFER_BYTES, Entry, MANIFEST_FILE,
    MANIFEST_VERSION, Manifest, Origin, RestoreReport, check_backup, is_database_path, is_vacant,
    restore, restore_until,
};
pub use error::{DatabaseError, Result, Unfinished};
pub use inspect::{Inspection, inspect};
pub use lock::{DatabaseLock, METADATA_DIR};
pub use marker::{FORMAT_FILE, RESTORING_FILE, encode as encode_marker};
pub use migrate::{MIGRATIONS, MigrateOptions, Migration, MigrationReport, migrate, migrate_with};
pub use version::{
    CURRENT_VERSION, LEGACY_VERSION, MIGRATION_FILE, OLDEST_MIGRATED, OLDEST_READ, check_openable,
    judge, stamp, store_version,
};
