//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing a whole log from a list of records, and reading one without
//! changing it: the halves a backup and an offline check need.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use super::{frame, io, replay};
use crate::error::MetadataError;
use crate::op::Op;

/// Writes a log holding exactly `ops` at `path`, at this build's version,
/// replacing any file there, and fsyncs it. The file's name is not made
/// durable: the caller renames it into place and syncs that directory.
pub(crate) fn write_log(path: &Path, ops: &[Op]) -> Result<File, MetadataError> {
    let f = File::create(path).map_err(io("create", path))?;
    let mut out = BufWriter::new(f);
    out.write_all(&replay::header())
        .map_err(io("write", path))?;
    let mut buf = Vec::new();
    for op in ops {
        buf.clear();
        frame(op, &mut buf);
        out.write_all(&buf).map_err(io("write", path))?;
    }
    let f = out
        .into_inner()
        .map_err(|e| io("write", path)(e.into_error()))?;
    f.sync_all().map_err(io("fsync", path))?;
    Ok(f)
}

/// What [`inspect_log`] found.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogInspection {
    /// Records that replay, batch markers included.
    pub records: u64,
    /// The header's version: 1 or 2 (docs/reference/compatibility.md).
    pub version: u32,
    /// Bytes after the last whole record: a torn, unacknowledged write. Opening
    /// the log would cut them into a side file; this only counts them.
    pub torn_bytes: u64,
}

/// Reads the log in `dir` and checks every record, changing nothing: no lock is
/// taken, no torn tail is cut and no old header is upgraded, so it is safe on a
/// stopped database and on a backup's copy. Copies the file into memory, as
/// opening does.
///
/// # Errors
/// [`MetadataError::Io`] (a missing log included); [`MetadataError::Corrupt`]
/// for damage before the last record; [`MetadataError::UnsupportedVersion`]
/// for a log from a newer Norn.
pub fn inspect_log(dir: &Path) -> Result<LogInspection, MetadataError> {
    let path = dir.join("metadata.log");
    let bytes = fs::read(&path).map_err(io("read", &path))?;
    let (end, records, version) = replay::scan(&bytes, &path, |_| {})?;
    Ok(LogInspection {
        records,
        version,
        torn_bytes: bytes.len().saturating_sub(end) as u64,
    })
}
