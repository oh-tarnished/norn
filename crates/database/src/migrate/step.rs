//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One migration step: new files beside the old, an atomic switch, a check, cleanup.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use norn_storage::sync_dir;

use super::Migration;
use super::files::{database_files, drop_derived};
use super::record::{Phase, Record};
use crate::error::{DatabaseError, Result};
use crate::inspect::inspect;
use crate::marker;
use crate::version::MIGRATION_FILE;

/// A test's chance to stop the job here, as a crash would.
pub(super) type Stop<'a> = &'a dyn Fn(&str) -> bool;

pub(super) fn halt(stop: Stop<'_>, point: &str) -> Result<()> {
    if stop(point) {
        return Err(DatabaseError::Interrupted);
    }
    Ok(())
}

fn beside(root: &Path, file: &str, suffix: &str) -> PathBuf {
    let mut name = root.join(file).into_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn new_name(root: &Path, file: &str, record: &Record) -> PathBuf {
    beside(root, file, &format!(".v{}-new", record.to))
}

fn old_name(root: &Path, file: &str, record: &Record) -> PathBuf {
    beside(root, file, &format!(".v{}-old", record.from))
}

fn sync_parents(root: &Path, files: &[String]) -> Result<()> {
    let mut dirs: Vec<PathBuf> = files
        .iter()
        .filter_map(|f| root.join(f).parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        sync_dir(&dir)?;
    }
    Ok(())
}

/// Rewrites what `step` applies to, beside the originals, and fsyncs each. A file the
/// step leaves as it is gets no new one. Reruns overwrite what an earlier run wrote.
fn prepare(root: &Path, step: &Migration, record: &mut Record, stop: Stop<'_>) -> Result<()> {
    let mut changed = Vec::new();
    for file in database_files(root)? {
        if !(step.applies)(&file) {
            continue;
        }
        halt(stop, &format!("prepare {file}"))?;
        let (from, to) = (root.join(&file), new_name(root, &file, record));
        if (step.rewrite)(&from, &to).map_err(|what| DatabaseError::corrupt(&from, what))? {
            let out = File::open(&to).map_err(|e| DatabaseError::io("open", &to, e))?;
            out.sync_all()
                .map_err(|e| DatabaseError::io("fsync", &to, e))?;
            changed.push(file);
        } else if to.exists() {
            fs::remove_file(&to).map_err(|e| DatabaseError::io("remove", &to, e))?;
        }
    }
    sync_parents(root, &changed)?;
    halt(stop, "commit")?;
    record.files = changed;
    record.phase = Phase::Swap;
    record.write(root)
}

/// Moves each new file into place, keeping the old one under another name. Each
/// rename is safe to find done: a file with no new one beside it has been switched.
fn swap(root: &Path, record: &mut Record, stop: Stop<'_>) -> Result<()> {
    for file in &record.files {
        halt(stop, &format!("swap {file}"))?;
        let (at, new) = (root.join(file), new_name(root, file, record));
        if !new.exists() {
            continue;
        }
        let old = old_name(root, file, record);
        if !old.exists() {
            fs::rename(&at, &old).map_err(|e| DatabaseError::io("rename", &at, e))?;
        }
        halt(stop, &format!("swap-new {file}"))?;
        fs::rename(&new, &at).map_err(|e| DatabaseError::io("rename", &new, e))?;
    }
    sync_parents(root, &record.files)?;
    if !record.files.is_empty() {
        drop_derived(root)?;
    }
    halt(stop, "mark")?;
    marker::write(root, record.to)?;
    record.phase = Phase::Finish;
    record.write(root)
}

/// Checks the result, then lets go of the old files. A check that fails leaves them,
/// and the record, in place: nothing has been lost, and the error says so.
fn finish(root: &Path, record: &Record, keep_old: bool, stop: Stop<'_>) -> Result<()> {
    halt(stop, "verify")?;
    let found = inspect(root)?;
    if found.damaged_regions > record.damage {
        return Err(DatabaseError::Damaged {
            root: root.to_path_buf(),
            regions: found.damaged_regions - record.damage,
        });
    }
    for file in &record.files {
        halt(stop, &format!("clean {file}"))?;
        let old = old_name(root, file, record);
        if !keep_old && old.exists() {
            fs::remove_file(&old).map_err(|e| DatabaseError::io("remove", &old, e))?;
        }
    }
    sync_parents(root, &record.files)?;
    halt(stop, "finish")?;
    let path = root.join(MIGRATION_FILE);
    fs::remove_file(&path).map_err(|e| DatabaseError::io("remove", &path, e))?;
    Ok(sync_dir(root)?)
}

/// Runs a step from wherever `record` says it got to. Returns the files it rewrote.
pub(super) fn run(
    root: &Path,
    step: &Migration,
    mut record: Record,
    keep_old: bool,
    stop: Stop<'_>,
) -> Result<usize> {
    if record.phase == Phase::Prepare {
        prepare(root, step, &mut record, stop)?;
    }
    if record.phase == Phase::Swap {
        swap(root, &mut record, stop)?;
    }
    finish(root, &record, keep_old, stop)?;
    Ok(record.files.len())
}
