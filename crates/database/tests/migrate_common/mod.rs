//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A test-only migration: store version 0 is, for these tests alone, a store whose
//! segments start with four extra bytes.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use norn_database::{MigrateOptions, Migration};

use super::support::{SEGMENT, database};

pub const OLD_PREFIX: &[u8; 4] = b"OLD0";

pub fn is_segment(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "nseg")
}

pub fn strip_prefix(from: &Path, to: &Path) -> Result<bool, String> {
    let bytes = fs::read(from).map_err(|e| e.to_string())?;
    let body = bytes.strip_prefix(OLD_PREFIX).ok_or("no old prefix")?;
    fs::write(to, body).map_err(|e| e.to_string())?;
    Ok(true)
}

pub const STEP: Migration = Migration {
    from: 0,
    to: 1,
    applies: is_segment,
    rewrite: strip_prefix,
};

/// A store at the test's version 0, and the bytes its segment must end up as.
pub fn old_store(root: &Path) -> Vec<u8> {
    database(root);
    let at = root.join(SEGMENT);
    let good = fs::read(&at).unwrap();
    let mut old = OLD_PREFIX.to_vec();
    old.extend_from_slice(&good);
    fs::write(&at, &old).unwrap();
    fs::write(root.join("FORMAT"), norn_database::encode_marker(0)).unwrap();
    good
}

pub const TO_1: MigrateOptions = MigrateOptions {
    to: 1,
    dry_run: false,
    keep_old: false,
};
