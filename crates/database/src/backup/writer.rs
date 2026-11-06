//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing a backup directory, one sealed file at a time.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

use norn_core::StreamId;
use norn_storage::sync_dir;

use super::copy::{copy_durable, is_vacant, measure, tail_of};
use super::manifest::{Entry, Manifest, Origin};
use super::summary::{BackupSummary, parent_path};
use crate::error::{DatabaseError, Result};
use crate::version::CURRENT_VERSION;

/// A log file's path inside a database and a backup.
const LOG_PATH: &str = "metadata/metadata.log";

/// A backup being written. Nothing about it counts until [`finish`](Self::finish)
/// writes the manifest: a directory without one is not a backup.
///
/// Memory: one copy buffer, and one [`Entry`] per file. Concurrency: one owner.
/// Failure: a failed or abandoned backup leaves its files and no manifest; nothing
/// removes them, because this never deletes under a path a client named.
#[derive(Debug)]
pub struct BackupWriter {
    dir: PathBuf,
    parent: Option<String>,
    base: HashMap<String, Entry>,
    files: Vec<Entry>,
    dirs: BTreeSet<PathBuf>,
    created_unix_nanos: i64,
    id: String,
    norn_version: String,
    summary: BackupSummary,
}

impl BackupWriter {
    /// Starts a backup in `dir`, which must be absolute and absent or empty, on top of
    /// the backup at `base` if given.
    ///
    /// # Errors
    /// [`DatabaseError::NotEmpty`]; [`DatabaseError::Io`]; as [`Manifest::read`] for
    /// the base, and [`DatabaseError::Corrupt`] for a relative path.
    pub fn begin(
        dir: &Path,
        base: Option<&Path>,
        created_unix_nanos: i64,
        id: &str,
    ) -> Result<Self> {
        if !matches!(
            dir.components().next(),
            Some(Component::RootDir | Component::Prefix(_))
        ) {
            return Err(DatabaseError::corrupt(
                dir,
                "a backup's path must be absolute",
            ));
        }
        if !is_vacant(dir)? {
            return Err(DatabaseError::NotEmpty { path: dir.into() });
        }
        let found = base.map(Manifest::read).transpose()?;
        fs::create_dir_all(dir).map_err(|e| DatabaseError::io("create", dir, e))?;
        let parent = base.map(|b| parent_path(dir, b)).transpose()?;
        let known = found.into_iter().flat_map(|m| m.files);
        Ok(Self {
            dir: dir.to_path_buf(),
            parent,
            base: known.map(|e| (e.path.clone(), e)).collect(),
            files: Vec::new(),
            dirs: BTreeSet::from([dir.to_path_buf()]),
            created_unix_nanos,
            id: id.to_owned(),
            norn_version: env!("CARGO_PKG_VERSION").to_owned(),
            summary: BackupSummary::default(),
        })
    }

    fn folder(&mut self, path: &str) -> Result<PathBuf> {
        let target = self.dir.join(path);
        if let Some(parent) = target.parent().map(Path::to_path_buf) {
            fs::create_dir_all(&parent).map_err(|e| DatabaseError::io("create", &parent, e))?;
            self.dirs.insert(parent);
        }
        Ok(target)
    }

    fn record(&mut self, path: String, len: u64, crc: u32, tail: u32, origin: Origin) {
        self.summary.files += 1;
        self.summary.bytes += len;
        if origin == Origin::Here {
            self.summary.copied_files += 1;
            self.summary.copied_bytes += len;
        }
        let entry = Entry {
            path,
            len,
            crc,
            tail,
            origin,
        };
        self.files.push(entry);
    }

    /// Adds a sealed segment of `stream`, as opaque bytes: an encrypted one needs no
    /// key. It is not copied when the base holds a file of this name, length and tail.
    ///
    /// # Errors
    /// [`DatabaseError::Io`]; [`DatabaseError::Corrupt`] for a file not named as a
    /// segment.
    pub fn add_segment(&mut self, stream: StreamId, segment: &Path) -> Result<()> {
        let name = segment.file_name().and_then(|n| n.to_str());
        let name = name.ok_or_else(|| DatabaseError::corrupt(segment, "not a segment's name"))?;
        let path = format!("streams/{:016x}/{name}", stream.get());
        if !super::manifest::is_database_path(&path) {
            return Err(DatabaseError::corrupt(segment, "not a segment's name"));
        }
        let (len, tail) = tail_of(segment)?;
        let reusable = self.base.get(&path);
        if let Some(known) = reusable.filter(|k| k.len == len && k.tail == tail) {
            let crc = known.crc;
            self.record(path, len, crc, tail, Origin::Parent);
            return Ok(());
        }
        let target = self.folder(&path)?;
        let (len, crc) = copy_durable(segment, &target)?;
        self.record(path, len, crc, tail, Origin::Here);
        Ok(())
    }

    /// Where to write the metadata log, which is not the live file but a copy of the
    /// catalogue at a commit point, before calling [`add_log`](Self::add_log).
    ///
    /// # Errors
    /// [`DatabaseError::Io`].
    pub fn log_path(&mut self) -> Result<PathBuf> {
        self.folder(LOG_PATH)
    }

    /// Records the log written at [`log_path`](Self::log_path).
    ///
    /// # Errors
    /// [`DatabaseError::Io`].
    pub fn add_log(&mut self) -> Result<()> {
        let path = self.dir.join(LOG_PATH);
        let (len, crc) = measure(&path)?;
        let (_, tail) = tail_of(&path)?;
        self.record(LOG_PATH.to_owned(), len, crc, tail, Origin::Here);
        Ok(())
    }

    /// Makes every name durable, then writes the manifest: the moment it is a backup.
    ///
    /// # Errors
    /// [`DatabaseError::Storage`], when a directory or the manifest cannot be synced.
    pub fn finish(self) -> Result<BackupSummary> {
        for dir in self.dirs.iter().rev() {
            sync_dir(dir)?;
        }
        let manifest = Manifest {
            id: self.id,
            created_unix_nanos: self.created_unix_nanos,
            store_version: CURRENT_VERSION,
            norn_version: self.norn_version,
            parent: self.parent,
            files: self.files,
        };
        manifest.write(&self.dir)?;
        Ok(self.summary)
    }
}
