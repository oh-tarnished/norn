//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! File I/O helpers: positioned reads that count bytes, and atomic replace.
//!
//! Reads are plain `seek` + `read_exact`, not memory maps: mapping needs
//! `unsafe` (a mapped file can change under the reader) and this crate has
//! none. mmap remains a later, measured option (docs/reference/storage.md).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{Result, StorageError};
use crate::platform::sync_dir;

/// A read-only file that counts the bytes it reads, so tests and metrics
/// (SPEC §34 `bytes_read`) see exactly what a partial read touched.
///
/// Concurrency: `&mut self` for every read; one owner at a time.
#[derive(Debug)]
pub(crate) struct Source {
    file: File,
    path: PathBuf,
    len: u64,
    read: u64,
}

impl Source {
    /// Opens `path` for reading and records its length.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| StorageError::io("open", path, e))?;
        let len = file
            .metadata()
            .map_err(|e| StorageError::io("stat", path, e))?
            .len();
        Ok(Self {
            file,
            path: path.to_path_buf(),
            len,
            read: 0,
        })
    }

    /// Fills `buf` from file offset `offset`.
    pub(crate) fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let io = |e| StorageError::io("read", &self.path, e);
        self.file.seek(SeekFrom::Start(offset)).map_err(io)?;
        self.file
            .read_exact(buf)
            .map_err(|e| StorageError::io("read", &self.path, e))?;
        self.read = self.read.saturating_add(buf.len() as u64);
        Ok(())
    }

    /// Reads `len` bytes at `offset` into `buf`, replacing its contents.
    pub(crate) fn read_vec(&mut self, offset: u64, len: usize, buf: &mut Vec<u8>) -> Result<()> {
        buf.clear();
        buf.resize(len, 0);
        self.read_at(offset, buf)
    }

    /// The file's length when opened.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// The file's path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Bytes read so far.
    pub(crate) fn bytes_read(&self) -> u64 {
        self.read
    }
}

/// Replaces `path` with `bytes` atomically: write `path.tmp`, fsync it,
/// rename over `path`, fsync the directory. A crash leaves either the old
/// file or the new one, never a mix; a stray `.tmp` is the only residue.
///
/// # Errors
/// [`StorageError::Io`] from any step; on error `path` is unchanged.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|e| StorageError::io("create", &tmp, e))?;
    f.write_all(bytes)
        .map_err(|e| StorageError::io("write", &tmp, e))?;
    f.sync_all()
        .map_err(|e| StorageError::io("fsync", &tmp, e))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| StorageError::io("rename", &tmp, e))?;
    sync_dir(parent(path))
}

/// `path` with `.tmp` appended: the name every in-progress write uses.
pub(crate) fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// The directory holding `path` (`.` for a bare name).
pub(crate) fn parent(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}
