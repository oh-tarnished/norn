//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The migration path: what it refuses, and what the daemon decides.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod migrate_common;
mod support;

use std::cell::Cell;
use std::fs;
use std::path::Path;

use migrate_common::{STEP, TO_1, old_store};
use norn_database::{
    CURRENT_VERSION, DatabaseError, DatabaseLock, MIGRATION_FILE, MigrateOptions, Migration,
    check_openable, judge, migrate, migrate_with, store_version,
};
use support::{Dir, SEGMENT, database};

#[test]
fn a_dry_run_changes_nothing() {
    let dir = Dir::new();
    old_store(&dir.0);
    let before = fs::read(dir.0.join(SEGMENT)).unwrap();
    let asked = Cell::new(0);
    let options = MigrateOptions {
        dry_run: true,
        ..TO_1
    };
    let report = migrate_with(&dir.0, options, &[STEP], &|_| {
        asked.set(asked.get() + 1);
        false
    })
    .unwrap();
    assert_eq!((report.steps, report.files, asked.get()), (1, 1, 0));
    assert_eq!(fs::read(dir.0.join(SEGMENT)).unwrap(), before);
    assert_eq!(store_version(&dir.0).unwrap(), 0);
    assert!(!dir.0.join(MIGRATION_FILE).exists());
}

#[test]
fn it_refuses_a_database_the_daemon_holds() {
    let dir = Dir::new();
    database(&dir.0);
    let held = DatabaseLock::acquire(&dir.0, false).unwrap();
    let refused = migrate(&dir.0, TO_1);
    assert!(
        matches!(refused, Err(DatabaseError::Locked { .. })),
        "{refused:?}"
    );
    drop(held);
    migrate(&dir.0, TO_1).unwrap();
}

#[test]
fn the_identity_records_the_version_of_a_store_written_before_format_existed() {
    let dir = Dir::new();
    database(&dir.0);
    assert!(!dir.0.join("FORMAT").exists());
    let report = migrate(&dir.0, TO_1).unwrap();
    assert_eq!((report.from, report.to, report.steps), (1, 1, 0));
    assert!(dir.0.join("FORMAT").exists());
    assert_eq!(report.verified_segments, 1);
    assert_eq!(report.damaged_regions, 0);
    assert_eq!(
        migrate(&dir.0, TO_1).unwrap().steps,
        0,
        "repeating is harmless"
    );
}

#[test]
fn no_chain_means_no_migration_and_nothing_goes_down() {
    let dir = Dir::new();
    old_store(&dir.0);
    let up = MigrateOptions { to: 9, ..TO_1 };
    assert!(matches!(
        migrate_with(&dir.0, up, &[STEP], &|_| false),
        Err(DatabaseError::NoMigration { .. })
    ));
    migrate_with(&dir.0, TO_1, &[STEP], &|_| false).unwrap();
    let down = MigrateOptions { to: 0, ..TO_1 };
    assert!(matches!(
        migrate(&dir.0, down),
        Err(DatabaseError::NoMigration { .. })
    ));
    assert_eq!(store_version(&dir.0).unwrap(), 1);
}

#[test]
fn a_step_that_damages_data_fails_and_keeps_the_old_files() {
    fn ruin(from: &Path, to: &Path) -> Result<bool, String> {
        let mut bytes = fs::read(from).map_err(|e| e.to_string())?;
        let at = bytes.len() / 2;
        bytes[at] ^= 0x10;
        fs::write(to, bytes).map_err(|e| e.to_string())?;
        Ok(true)
    }
    let dir = Dir::new();
    database(&dir.0);
    fs::write(dir.0.join("FORMAT"), norn_database::encode_marker(0)).unwrap();
    let before = fs::read(dir.0.join(SEGMENT)).unwrap();
    let step = Migration {
        rewrite: ruin,
        ..STEP
    };
    let result = migrate_with(&dir.0, TO_1, &[step], &|_| false);
    assert!(
        matches!(result, Err(DatabaseError::Damaged { .. })),
        "{result:?}"
    );
    assert_eq!(
        fs::read(dir.0.join(format!("{SEGMENT}.v0-old"))).unwrap(),
        before
    );
    assert!(
        check_openable(&dir.0).is_err(),
        "the daemon must not serve it"
    );
}

#[test]
fn the_daemons_decision_for_each_version() {
    let root = Path::new("/d");
    let v = CURRENT_VERSION;
    judge(root, v, v, v).unwrap();
    judge(root, v - 1, v - 1, v - 1).unwrap();
    let needs = judge(root, v - 1, v, v - 1).unwrap_err();
    assert!(matches!(needs, DatabaseError::NeedsMigration { .. }));
    assert!(
        needs
            .to_string()
            .contains("run nornctl migrate --data-dir /d --to 1"),
        "{needs}"
    );
    assert!(matches!(
        judge(root, v - 1, v, v),
        Err(DatabaseError::TooOld { .. })
    ));
    assert!(matches!(
        judge(root, v + 1, v, v),
        Err(DatabaseError::TooNew { .. })
    ));
}
