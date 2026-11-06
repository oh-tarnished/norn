//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The migration path, driven by a test-only step: what it does when it runs whole.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod migrate_common;
mod support;

use std::fs;

use migrate_common::{STEP, TO_1, old_store};
use norn_database::{
    DatabaseError, MIGRATION_FILE, MigrateOptions, Unfinished, check_openable, inspect,
    migrate_with, store_version,
};
use support::{Dir, SEGMENT};

#[test]
fn a_store_goes_up_a_version_and_the_old_files_go_only_at_the_end() {
    let dir = Dir::new();
    let good = old_store(&dir.0);
    let report = migrate_with(&dir.0, TO_1, &[STEP], &|_| false).unwrap();
    assert_eq!(
        (report.from, report.to, report.steps, report.files),
        (0, 1, 1, 1)
    );
    assert_eq!(fs::read(dir.0.join(SEGMENT)).unwrap(), good);
    assert_eq!(store_version(&dir.0).unwrap(), 1);
    assert!(!dir.0.join(MIGRATION_FILE).exists());
    assert!(!dir.0.join(format!("{SEGMENT}.v0-old")).exists());
    assert!(!dir.0.join(format!("{SEGMENT}.v1-new")).exists());
    assert_eq!(inspect(&dir.0).unwrap().verified_segments, 1);
    check_openable(&dir.0).unwrap();
}

#[test]
fn keep_old_leaves_the_way_back_beside_the_new_file() {
    let dir = Dir::new();
    let good = old_store(&dir.0);
    let before = fs::read(dir.0.join(SEGMENT)).unwrap();
    let options = MigrateOptions {
        keep_old: true,
        ..TO_1
    };
    migrate_with(&dir.0, options, &[STEP], &|_| false).unwrap();
    assert_eq!(fs::read(dir.0.join(SEGMENT)).unwrap(), good);
    assert_eq!(
        fs::read(dir.0.join(format!("{SEGMENT}.v0-old"))).unwrap(),
        before
    );
    assert_eq!(
        inspect(&dir.0).unwrap().verified_segments,
        1,
        "the old file confused a reader"
    );
}

#[test]
fn a_run_killed_at_any_point_finishes_when_repeated() {
    // First find every stop point, then die at each.
    let seen = std::cell::RefCell::new(Vec::new());
    let probe = Dir::new();
    old_store(&probe.0);
    migrate_with(&probe.0, TO_1, &[STEP], &|p| {
        seen.borrow_mut().push(p.to_owned());
        false
    })
    .unwrap();
    let points = seen.into_inner();
    assert!(points.len() >= 7, "{points:?}");
    for point in &points {
        let dir = Dir::new();
        let good = old_store(&dir.0);
        let stopped = migrate_with(&dir.0, TO_1, &[STEP], &|p| p == point);
        assert!(
            matches!(stopped, Err(DatabaseError::Interrupted)),
            "{point}: {stopped:?}"
        );
        let refused = check_openable(&dir.0);
        assert!(
            matches!(
                refused,
                Err(DatabaseError::Unfinished {
                    job: Unfinished::Migration,
                    ..
                })
            ),
            "{point}: the daemon must refuse: {refused:?}"
        );
        // Dying twice in a row at the same place is no different.
        let again = migrate_with(&dir.0, TO_1, &[STEP], &|p| p == point);
        assert!(
            matches!(again, Err(DatabaseError::Interrupted)),
            "{point}: {again:?}"
        );
        let done = migrate_with(&dir.0, TO_1, &[STEP], &|_| false).unwrap();
        assert!(done.resumed, "{point}");
        assert_eq!(fs::read(dir.0.join(SEGMENT)).unwrap(), good, "{point}");
        assert_eq!(store_version(&dir.0).unwrap(), 1, "{point}");
        assert!(!dir.0.join(format!("{SEGMENT}.v0-old")).exists(), "{point}");
        assert!(!dir.0.join(MIGRATION_FILE).exists(), "{point}");
        check_openable(&dir.0).unwrap();
    }
}
