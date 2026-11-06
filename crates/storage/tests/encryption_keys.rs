//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Keys: the wrong one, none, rotation, and recovery under them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;
mod crypt_common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{STREAM, Scratch, chunk};
use crypt_common::{A, B, opts};
use norn_storage::{
    ENCRYPTION_FILE, EncryptionError, KeyError, Keyring, RecoveryMode, SegmentHeader,
    SegmentReader, SegmentWriter, StorageError, StorageOptions, check_database, recover_segment,
    recover_stream, stream_dir,
};

/// A database at `root` with one sealed segment numbered `seq`, under `o`.
fn seeded(root: &Path, seq: u64, o: StorageOptions) -> PathBuf {
    let dir = stream_dir(root, STREAM);
    fs::create_dir_all(&dir).unwrap();
    let mut w = SegmentWriter::create(&dir, SegmentHeader::new(STREAM, seq), o).unwrap();
    w.append(&chunk(1, seq as i64 * 100, 50)).unwrap();
    w.seal().unwrap().path
}

#[test]
fn the_wrong_key_is_named_as_such_and_nothing_changes() {
    let s = Scratch::new();
    let path = seeded(s.path(), 0, opts(A));
    let before = fs::read(&path).unwrap();
    let wrong = opts(B);
    let Err(StorageError::Encryption(e)) = SegmentReader::open(&path, wrong) else {
        panic!("must be refused")
    };
    assert!(matches!(e, EncryptionError::KeyUnavailable { .. }));
    let text = e.to_string();
    assert!(text.contains("wrong key"), "{text}");
    assert!(!text.contains(B) && !text.contains(A), "{text}");
    let dir = stream_dir(s.path(), STREAM);
    assert!(matches!(
        recover_stream(&dir, &wrong, RecoveryMode::Repair),
        Err(StorageError::Encryption(
            EncryptionError::KeyUnavailable { .. }
        ))
    ));
    assert!(matches!(
        check_database(s.path(), wrong.keyring),
        Err(StorageError::Encryption(
            EncryptionError::KeyUnavailable { .. }
        ))
    ));
    assert_eq!(before, fs::read(&path).unwrap());
    assert!(!dir.join("quarantine").exists(), "not treated as damage");
}

#[test]
fn a_plain_store_with_keys_and_an_encrypted_one_without_are_refused_both_ways() {
    let (plain, sealed) = (Scratch::new(), Scratch::new());
    seeded(plain.path(), 0, StorageOptions::default());
    seeded(sealed.path(), 0, opts(A));
    let on = opts(A).keyring;
    let text = |r: Result<(), StorageError>| r.unwrap_err().to_string();
    assert!(text(check_database(plain.path(), on)).contains("not encrypted"));
    assert!(!plain.path().join(ENCRYPTION_FILE).exists());
    assert!(text(check_database(sealed.path(), None)).contains("is encrypted"));
    assert!(
        check_database(plain.path(), None).is_ok(),
        "plain stays as it was"
    );
}

#[test]
fn an_empty_store_opened_encrypted_is_marked_and_refused_without_keys_after() {
    let s = Scratch::new();
    check_database(s.path(), opts(A).keyring).unwrap();
    assert!(s.path().join(ENCRYPTION_FILE).exists());
    assert!(check_database(s.path(), None).is_err());
    assert!(
        check_database(s.path(), opts(B).keyring).is_ok(),
        "no segment needs A"
    );
}

#[test]
fn recovery_seals_an_encrypted_segment_a_crash_left_with_a_torn_tail() {
    let (s, o) = (Scratch::new(), opts(A));
    let mut w = SegmentWriter::create(s.path(), SegmentHeader::new(STREAM, 0), o).unwrap();
    w.append(&chunk(1, 0, 50)).unwrap();
    let second = w.append(&chunk(2, 0, 50)).unwrap();
    w.sync().unwrap();
    let path = w.path().to_path_buf();
    drop(w);
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(second.offset + 60).unwrap();
    drop(file);
    let done = recover_segment(&path, &o, RecoveryMode::Repair).unwrap();
    assert!(done.events.iter().any(|e| e.is_corruption()));
    let mut r = SegmentReader::open(&path, o).unwrap();
    assert!(r.is_sealed());
    assert_eq!(r.chunks().len(), 1);
    let meta = r.chunks()[0];
    assert_eq!(r.read_chunk(&meta).unwrap().len(), 50);
    assert_eq!(
        recover_segment(&path, &o, RecoveryMode::ReportOnly)
            .unwrap()
            .events
            .len(),
        0
    );
}

#[test]
fn key_text_is_parsed_strictly_and_errors_hold_no_key() {
    assert_eq!(Keyring::parse("# nothing\n"), Err(KeyError::Empty));
    let short = Keyring::parse("abcd").unwrap_err();
    assert_eq!(short, KeyError::Malformed { line: 1 });
    assert!(!short.to_string().contains("abcd"));
    assert_eq!(
        Keyring::parse(&format!("{A}\n{A}")),
        Err(KeyError::Duplicate { line: 2 })
    );
    let keys = Keyring::parse(&format!("{B} # new\n{A}")).unwrap();
    assert_eq!(keys.ids().count(), 2);
    assert!(!format!("{keys:?}").contains(B));
}

#[cfg(unix)]
#[test]
fn a_key_file_others_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    let path = s.path().join("key");
    fs::write(&path, A).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Keyring::from_file(&path),
        Err(KeyError::Insecure { mode: 0o644, .. })
    ));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Keyring::from_file(&path).is_ok());
}
