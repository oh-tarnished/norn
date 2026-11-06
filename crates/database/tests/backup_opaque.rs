//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup treats a segment as bytes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::fs;

use norn_core::StreamId;
use norn_database::{BackupWriter, check_backup};
use support::{Dir, SEGMENT};

#[test]
fn a_segment_goes_into_a_backup_unread_so_an_encrypted_one_needs_no_key() {
    let dir = Dir::new();
    let opaque = dir.0.join("0000000000000000-g0000.nseg");
    // Not a segment any reader could parse: a backup must not care.
    fs::write(
        &opaque,
        b"NORNSEG\0\x01\x01ciphertext, as far as this crate knows",
    )
    .unwrap();
    let b = dir.0.join("backup");
    let mut w = BackupWriter::begin(&b, None, 7, "test").unwrap();
    w.add_segment(StreamId::new(1), &opaque).unwrap();
    let summary = w.finish().unwrap();
    assert_eq!(summary.copied_files, 1);
    assert_eq!(
        fs::read(b.join(SEGMENT)).unwrap(),
        fs::read(&opaque).unwrap()
    );
    assert_eq!(check_backup(&b).unwrap().files, 1);
}
