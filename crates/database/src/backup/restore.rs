//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl restore`: a backup into an empty directory, offline, and restartable.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use norn_storage::{atomic_write, sync_dir};

use super::chain::Chain;
use super::copy::{copy_durable, measure};
use super::manifest::Entry;
use crate::error::{DatabaseError, Result};
use crate::inspect::inspect;
use crate::lock::DatabaseLock;
use crate::marker::{self, RESTORING_FILE};

/// What a finished restore wrote and found.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RestoreReport {
    /// Files in the database.
    pub files: u64,
    /// Bytes they take.
    pub bytes: u64,
    /// Of those, files a resumed restore found already finished.
    pub already_done: u64,
    /// Metadata records that replay.
    pub records: u64,
    /// Segments whose every chunk was read and checked.
    pub verified_segments: u64,
    /// Damaged regions those chunks hold. Every checksum in the backup passed, so
    /// these were damaged when they were backed up, and `verify` will say the same.
    pub damaged_regions: u64,
}

/// Whether `root` can take a new restore: absent, empty, or holding only the lock
/// file an interrupted start of one created.
fn is_fresh(root: &Path) -> Result<bool> {
    let list = |dir: &Path| -> Result<Vec<_>> {
        match fs::read_dir(dir) {
            Ok(all) => all
                .map(|e| e.map(|e| e.file_name()))
                .collect::<std::io::Result<_>>()
                .map_err(|e| DatabaseError::io("list", dir, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(DatabaseError::io("list", dir, e)),
        }
    };
    let top = list(root)?;
    Ok(match top.as_slice() {
        [] => true,
        [only] if only == crate::lock::METADATA_DIR => {
            let inner = list(&root.join(crate::lock::METADATA_DIR))?;
            matches!(inner.as_slice(), []) || matches!(inner.as_slice(), [f] if f == "LOCK")
        }
        _ => false,
    })
}

/// Puts one file in place: beside its name, checked, fsynced, then renamed. A file
/// already there with the right measure, from an interrupted run, stays.
fn place(chain: &Chain, entry: &Entry, root: &Path, dirs: &mut BTreeSet<PathBuf>) -> Result<bool> {
    let to = root.join(&entry.path);
    let want = (entry.len, entry.crc);
    if to.exists() && measure(&to)? == want {
        return Ok(true);
    }
    let parent = to.parent().unwrap_or(root).to_path_buf();
    fs::create_dir_all(&parent).map_err(|e| DatabaseError::io("create", &parent, e))?;
    let source = chain.source(entry)?;
    let tmp = to.with_extension("restoring");
    let found = copy_durable(&source, &tmp)?;
    if found != want {
        let _ = fs::remove_file(&tmp);
        return Err(DatabaseError::Mismatch {
            path: source,
            expected: want,
            found,
        });
    }
    fs::rename(&tmp, &to).map_err(|e| DatabaseError::io("rename", &to, e))?;
    dirs.insert(parent);
    Ok(false)
}

/// Restores the backup at `from` into `root`. As [`restore_until`] with no stop.
///
/// # Errors
/// As [`restore_until`].
pub fn restore(from: &Path, root: &Path) -> Result<RestoreReport> {
    restore_until(from, root, &|_| false)
}

/// Checks every manifest of `from`'s chain, then writes the database into `root`.
/// `RESTORING` is there from before the first file until after the last, so a daemon
/// refuses the directory until it is whole; repeating the command finishes the job
/// and copies only what is missing. Afterwards reads the result as `verify` would.
/// Nothing in `from` changes, and encrypted segments need no key.
///
/// Blocks on the disk throughout. `stop` is asked before each file, and answering
/// true ends the job as a crash would, for a test.
///
/// # Errors
/// [`DatabaseError::NotEmpty`] for a `root` holding anything but an interrupted
/// restore; [`DatabaseError::Locked`]; [`DatabaseError::TooNew`];
/// [`DatabaseError::Mismatch`] for a damaged file; [`DatabaseError::Corrupt`] and
/// [`DatabaseError::Io`]; [`DatabaseError::Interrupted`] when `stop` says so.
pub fn restore_until(
    from: &Path,
    root: &Path,
    stop: &dyn Fn(&str) -> bool,
) -> Result<RestoreReport> {
    let chain = Chain::load(from)?;
    let resuming = root.join(RESTORING_FILE).exists();
    if !resuming && !is_fresh(root)? {
        return Err(DatabaseError::NotEmpty { path: root.into() });
    }
    let lock = DatabaseLock::acquire(root, true)?;
    atomic_write(&root.join(RESTORING_FILE), b"nornctl restore\n")?;
    let mut report = RestoreReport::default();
    let mut dirs = BTreeSet::from([root.to_path_buf()]);
    let head = chain
        .head()
        .ok_or_else(|| DatabaseError::corrupt(from, "no manifest"))?;
    for entry in &head.files {
        if stop(&entry.path) {
            return Err(DatabaseError::Interrupted);
        }
        report.already_done += u64::from(place(&chain, entry, root, &mut dirs)?);
        report.files += 1;
        report.bytes += entry.len;
    }
    for dir in dirs.iter().rev() {
        sync_dir(dir)?;
    }
    let found = inspect(root)?;
    report.records = found.records;
    report.verified_segments = found.verified_segments;
    report.damaged_regions = found.damaged_regions;
    marker::write(root, head.store_version)?;
    let restoring = root.join(RESTORING_FILE);
    fs::remove_file(&restoring).map_err(|e| DatabaseError::io("remove", &restoring, e))?;
    sync_dir(root)?;
    drop(lock);
    Ok(report)
}
