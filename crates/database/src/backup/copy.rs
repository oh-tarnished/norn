//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Moving a file as opaque bytes, measuring it as it goes.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use norn_storage::{crc32c, crc32c_extend};

use crate::error::{DatabaseError, Result};

/// The copy buffer: all the memory a copy takes.
pub const COPY_BUFFER_BYTES: usize = 256 * 1024;
/// How much of a file's end its tail checksum covers. A segment's directory lives
/// there, and the directory holds each chunk's CRC, so it stands for the content.
const TAIL_BYTES: u64 = 64 * 1024;

/// A file's measure: its length and CRC-32C.
pub type Measure = (u64, u32);

/// Reads `path` through, into `visit`, and measures it.
fn read_through(path: &Path, mut visit: impl FnMut(&[u8]) -> Result<()>) -> Result<Measure> {
    let mut from = File::open(path).map_err(|e| DatabaseError::io("open", path, e))?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let (mut len, mut crc) = (0u64, 0u32);
    loop {
        let n = from
            .read(&mut buffer)
            .map_err(|e| DatabaseError::io("read", path, e))?;
        let Some(part) = buffer.get(..n).filter(|p| !p.is_empty()) else {
            return Ok((len, crc));
        };
        crc = crc32c_extend(crc, part);
        len += n as u64;
        visit(part)?;
    }
}

/// The length and CRC-32C of `path`.
///
/// # Errors
/// [`DatabaseError::Io`].
pub fn measure(path: &Path) -> Result<Measure> {
    read_through(path, |_| Ok(()))
}

/// Copies `from` to a new `to`, fsyncs it, and returns its measure. The caller
/// syncs `to`'s directory, once, when it has them all.
///
/// # Errors
/// [`DatabaseError::Io`]. A partial `to` may remain; repeating replaces it.
pub fn copy_durable(from: &Path, to: &Path) -> Result<Measure> {
    let mut out = File::create(to).map_err(|e| DatabaseError::io("create", to, e))?;
    let measured = read_through(from, |part| {
        out.write_all(part)
            .map_err(|e| DatabaseError::io("write", to, e))
    })?;
    out.sync_all()
        .map_err(|e| DatabaseError::io("fsync", to, e))?;
    Ok(measured)
}

/// The CRC-32C of the last 64 KiB of `path` (all of it if shorter) and its length.
///
/// # Errors
/// [`DatabaseError::Io`].
pub fn tail_of(path: &Path) -> Result<Measure> {
    let io = |op, e| DatabaseError::io(op, path, e);
    let mut file = File::open(path).map_err(|e| io("open", e))?;
    let len = file.metadata().map_err(|e| io("stat", e))?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .map_err(|e| io("seek", e))?;
    let mut bytes = Vec::with_capacity(len.min(TAIL_BYTES) as usize);
    file.read_to_end(&mut bytes).map_err(|e| io("read", e))?;
    Ok((len, crc32c(&bytes)))
}

/// Whether `dir` holds nothing a job could overwrite: it is absent or empty.
///
/// # Errors
/// [`DatabaseError::Io`].
pub fn is_vacant(dir: &Path) -> Result<bool> {
    match fs::read_dir(dir) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(DatabaseError::io("list", dir, e)),
    }
}
