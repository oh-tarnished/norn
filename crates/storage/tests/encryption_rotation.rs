//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Key rotation: compaction under the new key, and `rekey`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod common;
mod crypt_common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{STREAM, Scratch, chunk};
use crypt_common::{A, B, forge, opts};
use norn_storage::{
    RecoveryMode, SegmentHeader, SegmentReader, SegmentWriter, StorageOptions, compact,
    recover_stream, rekey_database, stream_dir,
};

/// A database at `root` with one sealed segment numbered `seq`, under `o`.
fn seeded(root: &Path, seq: u64, o: StorageOptions) -> PathBuf {
    let dir = stream_dir(root, STREAM);
    fs::create_dir_all(&dir).unwrap();
    let mut w = SegmentWriter::create(&dir, SegmentHeader::new(STREAM, seq), o).unwrap();
    w.append(&chunk(1, seq as i64 * 100, 50)).unwrap();
    w.seal().unwrap().path
}

fn readable(path: &Path, o: StorageOptions) -> bool {
    let mut r = SegmentReader::open(path, o).unwrap();
    let metas = r.chunks().to_vec();
    metas.iter().all(|m| r.read_chunk(m).is_ok())
}

#[test]
fn compaction_writes_under_the_current_key_and_reads_the_older_one() {
    let s = Scratch::new();
    let inputs = vec![seeded(s.path(), 0, opts(A)), seeded(s.path(), 1, opts(A))];
    let both = opts(&format!("{B}\n{A}"));
    let dir = stream_dir(s.path(), STREAM);
    let done = compact(&dir, &inputs, &both).unwrap();
    assert!(
        readable(&done.output.path, opts(B)),
        "the older key is not needed"
    );
}

#[test]
fn rekey_moves_every_segment_to_the_current_key_keeping_offsets_and_lengths() {
    let s = Scratch::new();
    let path = seeded(s.path(), 0, opts(A));
    let before_len = fs::metadata(&path).unwrap().len();
    let before = SegmentReader::open(&path, opts(A))
        .unwrap()
        .chunks()
        .to_vec();
    let both = opts(&format!("{B}\n{A}"));
    assert!(
        readable(&path, both),
        "the older key still reads before the rewrite"
    );
    let report = rekey_database(s.path(), &both).unwrap();
    assert_eq!(
        (report.rewritten, report.current, report.left.len()),
        (1, 0, 0)
    );
    assert!(readable(&path, opts(B)), "only the new key is needed");
    assert!(
        SegmentReader::open(&path, opts(A)).is_err(),
        "the old key no longer opens it"
    );
    assert_eq!(before_len, fs::metadata(&path).unwrap().len());
    let after = SegmentReader::open(&path, opts(B))
        .unwrap()
        .chunks()
        .to_vec();
    assert_eq!(before, after);
    let again = rekey_database(s.path(), &both).unwrap();
    assert_eq!((again.rewritten, again.current), (0, 1));
}

#[test]
fn rekey_leaves_a_damaged_segment_under_its_key_and_says_so() {
    let s = Scratch::new();
    let path = seeded(s.path(), 0, opts(A));
    let meta = SegmentReader::open(&path, opts(A)).unwrap().chunks()[0];
    forge(&path, &meta, |frame| frame[60] ^= 1);
    let both = opts(&format!("{B}\n{A}"));
    let report = rekey_database(s.path(), &both).unwrap();
    assert_eq!((report.rewritten, report.left.len()), (0, 1));
    assert!(SegmentReader::open(&path, opts(A)).is_ok(), "still under A");
    let tmp = format!("{}.tmp", path.display());
    assert!(!Path::new(&tmp).exists());
}

#[test]
fn a_crash_mid_rekey_leaves_the_original_and_recovery_removes_the_rest() {
    let s = Scratch::new();
    let path = seeded(s.path(), 0, opts(A));
    let tmp = format!("{}.tmp", path.display());
    fs::write(&tmp, b"half a rewrite").unwrap();
    let dir = stream_dir(s.path(), STREAM);
    let both = opts(&format!("{B}\n{A}"));
    recover_stream(&dir, &both, RecoveryMode::Repair).unwrap();
    assert!(!Path::new(&tmp).exists());
    assert!(readable(&path, both));
}
