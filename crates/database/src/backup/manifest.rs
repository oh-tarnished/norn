//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup's `MANIFEST`: what it holds, written last, so its presence means whole.
//!
//! Text, one record a line, readable with `cat`; the last line is the CRC-32C of
//! everything before it, so a truncated or edited manifest is never believed:
//!
//! ```text
//! norn-backup 1
//! id nightly-3
//! created_unix_nanos 1700000000000000000
//! store_version 1
//! norn_version 1.0.0-rc.1
//! parent ../monday
//! file metadata/metadata.log 1050 5e1f00aa 5e1f00aa here
//! file streams/0000000000000001/0000000000000000-g0000.nseg 1007 0a0b0c0d 0a0b0c0d parent
//! crc 12345678
//! ```
//!
//! `file` is the path inside the database, its length, the CRC-32C of all of it, the
//! CRC-32C of its last 64 KiB, and where its bytes are: in this directory, or in
//! `parent`'s, for what an incremental backup did not copy again.

use std::fmt::Write as _;
use std::path::Path;

use norn_storage::{atomic_write, crc32c};

use crate::error::{DatabaseError, Result};

/// The manifest's file name inside a backup.
pub const MANIFEST_FILE: &str = "MANIFEST";
/// The manifest layout this build writes, and the newest it reads.
pub const MANIFEST_VERSION: u16 = 1;

/// Where a backed-up file's bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// In this backup's directory.
    Here,
    /// In the parent backup's, or further up its chain.
    Parent,
}

/// One file of a backup.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path inside a database, with `/` separators. Always checked by
    /// [`is_database_path`]: a manifest cannot name a file outside the database.
    pub path: String,
    /// Its length in bytes.
    pub len: u64,
    /// The CRC-32C of all of it.
    pub crc: u32,
    /// The CRC-32C of its last 64 KiB: an incremental's quick check that a file of
    /// the same name and length is the same file.
    pub tail: u32,
    /// Where its bytes are.
    pub origin: Origin,
}

/// A parsed manifest.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The ID its creator gave it.
    pub id: String,
    /// When the backup was taken, on the daemon's clock.
    pub created_unix_nanos: i64,
    /// The store version of the database backed up.
    pub store_version: u16,
    /// The release that took it, for people.
    pub norn_version: String,
    /// The backup an incremental builds on, relative to this one if not absolute.
    pub parent: Option<String>,
    /// Every file of the database at the cut, whichever backup holds its bytes.
    pub files: Vec<Entry>,
}

/// Whether `path` is a file a backup can hold: `metadata/metadata.log`, or
/// `streams/<16 hex digits>/<segment>.nseg`. Nothing else is ever restored, so no
/// manifest, damaged or crafted, can write outside the database or over its lock.
#[must_use]
pub fn is_database_path(path: &str) -> bool {
    if path == "metadata/metadata.log" {
        return true;
    }
    let mut parts = path.split('/');
    let (Some("streams"), Some(id), Some(name), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    id.len() == 16
        && id.bytes().all(|b| b.is_ascii_hexdigit())
        && norn_storage::SegmentName::parse(Path::new(name)).is_some()
}

impl Manifest {
    /// Its bytes, the final CRC line included.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut s = format!("norn-backup {MANIFEST_VERSION}\n");
        let _ = writeln!(s, "id {}", self.id);
        let _ = writeln!(s, "created_unix_nanos {}", self.created_unix_nanos);
        let _ = writeln!(s, "store_version {}", self.store_version);
        let _ = writeln!(s, "norn_version {}", self.norn_version);
        if let Some(parent) = &self.parent {
            let _ = writeln!(s, "parent {parent}");
        }
        for f in &self.files {
            let origin = match f.origin {
                Origin::Here => "here",
                Origin::Parent => "parent",
            };
            let (path, len, crc, tail) = (&f.path, f.len, f.crc, f.tail);
            let _ = writeln!(s, "file {path} {len} {crc:08x} {tail:08x} {origin}");
        }
        let crc = crc32c(s.as_bytes());
        let _ = writeln!(s, "crc {crc:08x}");
        s
    }

    /// Reads and checks the manifest of the backup in `dir`. Copies.
    ///
    /// # Errors
    /// [`DatabaseError::Io`], a missing manifest included: a backup stopped before
    /// its last step has none; as [`decode`](Self::decode).
    pub fn read(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST_FILE);
        let bytes = std::fs::read(&path).map_err(|e| DatabaseError::io("read", &path, e))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| DatabaseError::corrupt(&path, "MANIFEST is not text"))?;
        Self::decode(&text, &path)
    }

    /// Writes it into `dir`, atomically and durably.
    ///
    /// # Errors
    /// [`DatabaseError::Storage`] from the atomic write.
    pub fn write(&self, dir: &Path) -> Result<()> {
        Ok(atomic_write(
            &dir.join(MANIFEST_FILE),
            self.encode().as_bytes(),
        )?)
    }
}
