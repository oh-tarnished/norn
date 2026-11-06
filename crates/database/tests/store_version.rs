//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A store version this build does not know.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::fs;

use norn_database::{DatabaseError, check_openable};
use support::Dir;

#[test]
fn a_store_version_this_build_does_not_know_is_refused_and_untouched() {
    let dir = Dir::new();
    fs::write(dir.0.join("FORMAT"), [0u8; 3]).unwrap();
    assert!(matches!(
        check_openable(&dir.0),
        Err(DatabaseError::Corrupt { .. })
    ));
    let newer = norn_database::encode_marker(99);
    fs::write(dir.0.join("FORMAT"), newer).unwrap();
    let refused = check_openable(&dir.0).unwrap_err();
    assert!(
        matches!(refused, DatabaseError::TooNew { found: 99, .. }),
        "{refused:?}"
    );
    assert_eq!(fs::read(dir.0.join("FORMAT")).unwrap(), newer);
}
