//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup's files: damage is detected, a restore resumes, nothing is parsed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use norn_core::StreamId;
use norn_database::{
    BackupWriter, DatabaseError, DatabaseLock, MANIFEST_FILE, Manifest, RESTORING_FILE, Unfinished,
    check_backup, check_openable, is_database_path, restore, restore_until, store_version,
};
use support::{Dir, SEGMENT, database};

fn backup_of(root: &Path, to: &Path) {
    let mut w = BackupWriter::begin(to, None, 7, "test").unwrap();
    let log = w.log_path().unwrap();
    fs::copy(root.join("metadata/metadata.log"), log).unwrap();
    w.add_log().unwrap();
    w.add_segment(StreamId::new(1), &root.join(SEGMENT))
        .unwrap();
    w.finish().unwrap();
}

fn fresh() -> (Dir, PathBuf) {
    let dir = Dir::new();
    let (db, b) = (dir.0.join("db"), dir.0.join("backup"));
    database(&db);
    backup_of(&db, &b);
    (dir, b)
}

#[test]
fn a_restore_gives_the_same_bytes_and_a_marked_store() {
    let (dir, b) = fresh();
    let target = dir.0.join("restored");
    let report = restore(&b, &target).unwrap();
    assert_eq!(report.files, 2);
    assert_eq!(report.verified_segments, 1);
    assert_eq!(report.damaged_regions, 0);
    assert_eq!(
        fs::read(target.join(SEGMENT)).unwrap(),
        fs::read(dir.0.join("db").join(SEGMENT)).unwrap()
    );
    assert_eq!(store_version(&target).unwrap(), 1);
    assert!(!target.join(RESTORING_FILE).exists());
    check_openable(&target).unwrap();
}

#[test]
fn every_flipped_byte_of_a_segment_in_a_backup_is_detected() {
    let (dir, b) = fresh();
    let original = fs::read(b.join(SEGMENT)).unwrap();
    for at in (0..original.len()).step_by(37) {
        let mut bytes = original.clone();
        bytes[at] ^= 0x40;
        fs::write(b.join(SEGMENT), &bytes).unwrap();
        let checked = check_backup(&b);
        assert!(
            matches!(checked, Err(DatabaseError::Mismatch { .. })),
            "byte {at}"
        );
    }
    let mut bytes = original;
    bytes[100] ^= 1;
    fs::write(b.join(SEGMENT), &bytes).unwrap();
    let target = dir.0.join("restored");
    let result = restore(&b, &target);
    assert!(
        matches!(result, Err(DatabaseError::Mismatch { .. })),
        "{result:?}"
    );
    let refused = check_openable(&target);
    assert!(
        matches!(
            refused,
            Err(DatabaseError::Unfinished {
                job: Unfinished::Restore,
                ..
            })
        ),
        "a half-restored database must not be served: {refused:?}"
    );
}

#[test]
fn a_missing_file_or_a_cut_or_absent_manifest_is_detected() {
    let (_dir, b) = fresh();
    let text = fs::read_to_string(b.join(MANIFEST_FILE)).unwrap();
    fs::write(b.join(MANIFEST_FILE), &text[..text.len() - 20]).unwrap();
    assert!(matches!(
        check_backup(&b),
        Err(DatabaseError::Corrupt { .. })
    ));
    fs::write(
        b.join(MANIFEST_FILE),
        text.replace("store_version 1", "store_version 2"),
    )
    .unwrap();
    assert!(matches!(
        check_backup(&b),
        Err(DatabaseError::Corrupt { .. })
    ));
    fs::write(b.join(MANIFEST_FILE), &text).unwrap();
    check_backup(&b).unwrap();
    fs::remove_file(b.join(SEGMENT)).unwrap();
    assert!(matches!(check_backup(&b), Err(DatabaseError::Io { .. })));
    fs::remove_file(b.join(MANIFEST_FILE)).unwrap();
    assert!(matches!(check_backup(&b), Err(DatabaseError::Io { .. })));
}

#[test]
fn a_manifest_cannot_name_a_file_outside_the_database() {
    for bad in [
        "../../etc/passwd",
        "/etc/passwd",
        "metadata/LOCK",
        "streams/../../x.nseg",
        "streams/0000000000000001/../../../x.nseg",
        "streams/0000000000000001/journal/0000000000000000-g0000.nseg",
    ] {
        assert!(!is_database_path(bad), "{bad}");
    }
    assert!(is_database_path(SEGMENT));
    let m = "norn-backup 1\nfile ../../evil 1 00000000 00000000 here\n";
    let crc = norn_storage::crc32c(m.as_bytes());
    let text = format!("{m}crc {crc:08x}\n");
    let parsed = Manifest::decode(&text, Path::new("MANIFEST"));
    assert!(
        matches!(parsed, Err(DatabaseError::Corrupt { .. })),
        "{parsed:?}"
    );
}

#[test]
fn a_restore_stopped_anywhere_finishes_when_repeated_and_copies_only_what_is_missing() {
    let (dir, b) = fresh();
    let target = dir.0.join("restored");
    let stopped = restore_until(&b, &target, &|path| path == SEGMENT);
    assert!(
        matches!(stopped, Err(DatabaseError::Interrupted)),
        "{stopped:?}"
    );
    assert!(target.join(RESTORING_FILE).exists());
    assert!(check_openable(&target).is_err());
    let report = restore(&b, &target).unwrap();
    assert_eq!(report.already_done, 1);
    assert_eq!(report.files, 2);
    check_openable(&target).unwrap();
}

#[test]
fn a_restore_refuses_a_directory_in_use_or_holding_anything() {
    let (dir, b) = fresh();
    let used = dir.0.join("used");
    fs::create_dir_all(&used).unwrap();
    fs::write(used.join("precious"), b"x").unwrap();
    assert!(matches!(
        restore(&b, &used),
        Err(DatabaseError::NotEmpty { .. })
    ));
    assert_eq!(fs::read(used.join("precious")).unwrap(), b"x");
    let target = dir.0.join("held");
    let lock = DatabaseLock::acquire(&target, true).unwrap();
    assert!(matches!(
        restore(&b, &target),
        Err(DatabaseError::Locked { .. })
    ));
    drop(lock);
    restore(&b, &target).unwrap();
}
