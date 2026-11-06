//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Moving damaged bytes aside before anything is cut, and the cut itself.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::QUARANTINE_DIR;
use crate::error::{Result, StorageError};
use crate::fsio::{parent, tmp_path};
use crate::platform::{create_dir_durable, sync_dir};

/// The quarantine directory beside `segment`, created if missing.
fn dir_for(segment: &Path) -> Result<PathBuf> {
    let dir = parent(segment).join(QUARANTINE_DIR);
    create_dir_durable(&dir)?;
    Ok(dir)
}

fn file_name(segment: &Path) -> String {
    segment
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Copies bytes `offset..offset + len` of `segment` to
/// `quarantine/<segment name>.<offset:016x>.bad`, durably (temp, fsync,
/// rename, fsync directory), and returns the copy's path.
///
/// Memory: a fixed copy buffer, whatever `len` is. Never overwrites an earlier
/// quarantine file: a repeat after a crash at worst leaves a duplicate copy.
pub(super) fn copy_region(segment: &Path, offset: u64, len: u64) -> Result<PathBuf> {
    let dir = dir_for(segment)?;
    let dest = unused(&dir, &format!("{}.{offset:016x}.bad", file_name(segment)));
    let tmp = tmp_path(&dest);
    let mut src = File::open(segment).map_err(on("open", segment))?;
    src.seek(SeekFrom::Start(offset))
        .map_err(on("seek", segment))?;
    let mut out = File::create(&tmp).map_err(on("create", &tmp))?;
    let copied = io::copy(&mut src.take(len), &mut out).map_err(on("copy", &tmp))?;
    if copied != len {
        let e = io::Error::from(io::ErrorKind::UnexpectedEof);
        return Err(StorageError::io("copy", segment, e));
    }
    out.sync_all().map_err(on("fsync", &tmp))?;
    drop(out);
    fs::rename(&tmp, &dest).map_err(on("rename", &tmp))?;
    sync_dir(&dir)?;
    Ok(dest)
}

/// `dir/name`, or `dir/name.N` for the first free `N`. Quarantine only ever
/// grows: a sequence number can recur after its segment was moved here, and
/// that must not destroy the bytes kept the first time.
fn unused(dir: &Path, name: &str) -> PathBuf {
    let mut dest = dir.join(name);
    let mut n = 0u64;
    while dest.exists() {
        n += 1;
        dest = dir.join(format!("{name}.{n}"));
    }
    dest
}

/// Wraps an I/O error from `op` on `p`.
fn on<'a>(op: &'static str, p: &'a Path) -> impl FnOnce(io::Error) -> StorageError + 'a {
    move |e| StorageError::io(op, p, e)
}

/// Moves the whole of `segment` into quarantine and returns its new path.
pub(super) fn move_file(segment: &Path) -> Result<PathBuf> {
    let dir = dir_for(segment)?;
    let dest = unused(&dir, &file_name(segment));
    fs::rename(segment, &dest).map_err(|e| StorageError::io("rename", segment, e))?;
    sync_dir(&dir)?;
    sync_dir(parent(segment))?;
    Ok(dest)
}

/// Cuts `segment` to `len` bytes and fsyncs it. Only ever called after the
/// cut bytes are durably quarantined.
pub(super) fn truncate(segment: &Path, len: u64) -> Result<()> {
    let f = OpenOptions::new()
        .write(true)
        .open(segment)
        .map_err(|e| StorageError::io("open", segment, e))?;
    f.set_len(len)
        .map_err(|e| StorageError::io("truncate", segment, e))?;
    f.sync_all()
        .map_err(|e| StorageError::io("fsync", segment, e))
}

/// Removes `path` (a superseded or uncommitted segment) and fsyncs its
/// directory.
pub(super) fn remove(path: &Path) -> Result<()> {
    fs::remove_file(path).map_err(|e| StorageError::io("remove", path, e))?;
    sync_dir(parent(path))
}
