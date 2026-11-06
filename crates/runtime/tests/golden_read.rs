//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Stores written by 1.0 stay readable. If a change fails here, it broke every
//! database in the field: docs/reference/compatibility.md says what to do instead.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "golden_common/mod.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{STREAMS, copy_store, golden_root, open};
use norn_storage::{crc32c, list_segments, stream_dir};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    /// A private copy of a golden store: opening one repairs and upgrades it.
    fn of(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let unique = format!("norn-golden-read-{}-{name}-{n}", std::process::id());
        let path = std::env::temp_dir().join(unique);
        let _ = fs::remove_dir_all(&path);
        copy_store(&golden_root().join(name), &path);
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn segment_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            segment_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "nseg") {
            out.push(path);
        }
    }
}

#[test]
fn the_golden_files_are_the_bytes_that_were_checked_in() {
    let root = golden_root();
    let sums = fs::read_to_string(root.join("CHECKSUMS")).unwrap();
    assert!(sums.lines().count() > 8, "the golden stores are missing");
    for line in sums.lines() {
        let words: Vec<_> = line.split(' ').collect();
        let bytes = fs::read(root.join(words[0])).unwrap();
        assert_eq!(bytes.len().to_string(), words[1], "{}", words[0]);
        assert_eq!(format!("{:08x}", crc32c(&bytes)), words[2], "{}", words[0]);
    }
}

#[test]
fn every_golden_file_says_the_version_this_policy_promises() {
    let mut segments = Vec::new();
    segment_files(&golden_root(), &mut segments);
    assert!(segments.len() >= 4);
    for path in segments {
        let b = fs::read(&path).unwrap();
        assert_eq!(&b[0..8], b"NORNSEG\0", "{}", path.display());
        // Format 1.0: major 1, minor 0. Format 1.1 is only ever an encrypted one.
        assert_eq!(b[8..10], [1, 0], "{}", path.display());
        assert_eq!(
            b[10..12],
            1u16.to_le_bytes(),
            "schema of {}",
            path.display()
        );
        assert_eq!(b[13], 0, "flags of {}", path.display());
    }
    let log = fs::read(golden_root().join("clean/metadata/metadata.log")).unwrap();
    assert_eq!(&log[0..8], b"NORNMETA");
    assert_eq!(log[8..12], 2u32.to_le_bytes());
    let log = fs::read(golden_root().join("metadata-v1/metadata.log")).unwrap();
    assert_eq!(log[8..12], 1u32.to_le_bytes());
}

#[test]
fn a_clean_golden_store_opens_unrepaired_and_every_sample_reads_back() {
    let store = Scratch::of("clean");
    let db = open(&store.0);
    assert_eq!(db.recovery.corruptions, 0, "{:?}", db.recovery.findings);
    assert_eq!(db.recovery.replayed_samples, 0);
    assert_eq!(db.recovery.streams, 2);
    assert_eq!(
        db.recovery.clean_streams, 2,
        "the CLEAN markers were refused"
    );
    assert_eq!(
        db.recovery.index.rebuilt_streams, 0,
        "the index summaries were refused"
    );
    assert!(db.recovery.index.reused_segments >= 3);
    for stream in STREAMS {
        let id = db.stream(stream);
        assert_eq!(db.stored(&store.0, id), db.expected(stream, 2), "{stream}");
    }
    let merged = list_segments(&stream_dir(&store.0, db.stream("merged"))).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].header.generation, 1);
    db.ingest.close();
}

#[test]
fn a_golden_store_takes_new_samples_after_the_old_ones() {
    let store = Scratch::of("clean");
    let db = open(&store.0);
    db.write_session("plant", 2);
    let id = db.stream("plant");
    db.ingest.close();
    assert_eq!(db.stored(&store.0, id), db.expected("plant", 3));
}

#[test]
fn a_golden_crash_is_recovered_from_its_journal() {
    let store = Scratch::of("crash");
    let db = open(&store.0);
    assert_eq!(db.recovery.corruptions, 0, "{:?}", db.recovery.findings);
    assert_eq!(db.recovery.replayed_samples, 2 * 5 * common::PER_SESSION);
    db.ingest.close();
    for stream in STREAMS {
        let id = db.stream(stream);
        assert_eq!(db.stored(&store.0, id), db.expected(stream, 1), "{stream}");
    }
}

#[test]
fn a_version_1_metadata_log_opens_and_is_upgraded() {
    let store = Scratch::of("metadata-v1");
    let config = norn_core::Config::default();
    let dir = store.0.join("metadata");
    fs::create_dir_all(&dir).unwrap();
    fs::rename(store.0.join("metadata.log"), dir.join("metadata.log")).unwrap();
    let opened = norn_metadata::Registry::open(&dir, config.limits(), config.metadata());
    let (registry, _) = opened.unwrap();
    assert_eq!(registry.view().stream_ids().len(), 2);
    drop(registry);
    assert_eq!(norn_metadata::inspect_log(&dir).unwrap().version, 2);
}
