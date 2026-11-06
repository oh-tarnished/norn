//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Writes the golden stores. Only when asked: they pin a format, so changing them
//! is a decision, and one a reviewer sees (docs/reference/compatibility.md):
//!
//! ```text
//! NORN_WRITE_GOLDEN=1 cargo test -p norn-runtime --test golden_write -- --ignored
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "golden_common/mod.rs"]
mod common;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use common::{STREAMS, copy_store, declare, golden_root, open};
use norn_core::Config;
use norn_storage::{StorageOptions, crc32c, live_segments, stream_dir};

fn clean_store(root: &Path) {
    let first = open(root);
    declare(&first);
    for stream in STREAMS {
        first.write_session(stream, 0);
    }
    first.ingest.close();
    drop(first);
    let second = open(root);
    for stream in STREAMS {
        second.write_session(stream, 1);
    }
    let merged = stream_dir(root, second.stream("merged"));
    second.ingest.close();
    drop(second);
    let inputs: Vec<PathBuf> = live_segments(&merged)
        .unwrap()
        .into_iter()
        .map(|s| s.path)
        .collect();
    let opts = StorageOptions::from_config(&Config::default());
    norn_storage::compact(&merged, &inputs, &opts).unwrap();
    // One clean start and stop: inputs gone, markers and index summaries written.
    let last = open(root);
    last.ingest.close();
}

/// What a kill leaves: a durable batch in the journal, in no segment yet.
fn crash_store(root: &Path, into: &Path) {
    let live = open(root);
    declare(&live);
    live.write_session("plant", 0);
    live.write_session("merged", 0);
    copy_store(root, into);
    live.ingest.close();
}

fn checksums(root: &Path, dir: &Path, out: &mut String) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            checksums(root, &path, out);
        } else if path.file_name().is_some_and(|n| n != "CHECKSUMS") {
            let bytes = fs::read(&path).unwrap();
            let name = path.strip_prefix(root).unwrap().display().to_string();
            let crc = crc32c(&bytes);
            writeln!(out, "{name} {} {crc:08x}", bytes.len()).unwrap();
        }
    }
}

#[test]
#[ignore = "writes into the repository: run it to change the golden stores, on purpose"]
fn write_the_golden_stores() {
    if std::env::var_os("NORN_WRITE_GOLDEN").is_none() {
        return;
    }
    let golden = golden_root();
    let _ = fs::remove_dir_all(&golden);
    let scratch = std::env::temp_dir().join(format!("norn-golden-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    clean_store(&scratch.join("clean"));
    copy_store(&scratch.join("clean"), &golden.join("clean"));
    crash_store(&scratch.join("crash"), &golden.join("crash"));
    // A version 1 log: a log with no batch marker, under the old header.
    let mut log = fs::read(golden.join("crash/metadata/metadata.log")).unwrap();
    log[8..12].copy_from_slice(&1u32.to_le_bytes());
    let crc = crc32c(&log[..12]);
    log[12..16].copy_from_slice(&crc.to_le_bytes());
    fs::create_dir_all(golden.join("metadata-v1")).unwrap();
    fs::write(golden.join("metadata-v1/metadata.log"), log).unwrap();
    let mut sums = String::new();
    checksums(&golden, &golden, &mut sums);
    fs::write(golden.join("CHECKSUMS"), sums).unwrap();
    let _ = fs::remove_dir_all(&scratch);
}
