//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The clean-shutdown marker: which sealed segments need not be read at startup.
//!
//! Without it, every start would read every chunk of every stream to check its
//! CRCs, in time proportional to all stored data. A sealed segment is fsynced
//! before its seal and never changes, so what can be wrong with it after a crash
//! is what could be wrong with any byte on a disk at any time, which startup is
//! the wrong moment to look for: `nornctl verify` looks for that.
//!
//! So the marker is a list, written atomically by a daemon that has shut down
//! cleanly: these segments, of these lengths, were sealed and fsynced. It is
//! deliberately not deleted when the daemon starts, because it does not claim the
//! daemon stopped cleanly. It claims those segments were intact, which stays true:
//!
//! - A directory holding exactly what the marker lists, with no journal segment
//!   and no temporary file, was not written to since: nothing need be read.
//! - Anything else means later writes. What the marker lists is taken on trust and
//!   the rest is read and checked, however it got there: a segment rolled, a
//!   compaction's output, a journal.
//!
//! A stream with known damage has no marker, so it is read at every start and the
//! report repeats. See docs/reference/storage.md, "Clean shutdown".

mod wire;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use norn_core::{StreamId, Timestamp};

use crate::error::{Result, StorageError};
use crate::fsio::atomic_write;
use crate::segment::{SEGMENT_EXT, SegmentName};

/// The marker's file name in a stream directory.
pub const CLEAN_FILE: &str = "CLEAN";

/// What the marker keeps about one sealed segment: enough to tell it is the same
/// file, and to total the stream without opening it.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanSegment {
    /// The segment.
    pub name: SegmentName,
    /// Its file length.
    pub bytes: u64,
    /// The samples it holds.
    pub samples: u64,
    /// Its earliest and latest sample time; `None` when empty.
    pub time_range: Option<(Timestamp, Timestamp)>,
}

/// One stream's list of segments known sealed and intact.
///
/// Concurrency: owned plain data. Memory: about 60 bytes a segment while held.
/// Failure: a marker that cannot be read or fails its CRC vouches for nothing,
/// so it costs a full read and never a wrong answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanMarker {
    stream: StreamId,
    segments: HashMap<SegmentName, CleanSegment>,
}

impl CleanMarker {
    /// A marker for `stream` vouching for `segments`.
    #[must_use]
    pub fn new(stream: StreamId, segments: impl IntoIterator<Item = CleanSegment>) -> Self {
        let segments = segments.into_iter().map(|s| (s.name, s)).collect();
        Self { stream, segments }
    }

    /// The marker in stream directory `dir`, if there is a valid one for `stream`.
    /// Reads one small file. A bad one is treated as none and never reported: it
    /// is derived, so losing it costs time only.
    #[must_use]
    pub fn load(dir: &Path, stream: StreamId) -> Option<Self> {
        wire::load(&dir.join(CLEAN_FILE), stream).map(|all| Self::new(stream, all))
    }

    /// Writes this marker into stream directory `dir`, atomically and durably.
    ///
    /// # Errors
    /// [`StorageError::Io`]; the old marker, if any, is unchanged.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let mut sorted: Vec<_> = self.segments.values().copied().collect();
        sorted.sort_by_key(|s| s.name);
        atomic_write(&dir.join(CLEAN_FILE), &wire::encode(self.stream, &sorted))
    }

    /// Removes the marker from stream directory `dir`, durably, so that the next
    /// start reads the whole stream. Done as soon as damage is known.
    ///
    /// # Errors
    /// [`StorageError::Io`], other than for a marker already gone.
    pub fn remove(dir: &Path) -> Result<()> {
        let path = dir.join(CLEAN_FILE);
        match fs::remove_file(&path) {
            Ok(()) => crate::platform::sync_dir(dir),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::io("remove", path, e)),
        }
    }

    /// Whether this marker vouches for segment `name` at file length `len`.
    #[must_use]
    pub fn vouches(&self, name: SegmentName, len: u64) -> bool {
        self.segments.get(&name).is_some_and(|s| s.bytes == len)
    }

    /// The segments it lists, in no order.
    pub fn segments(&self) -> impl Iterator<Item = &CleanSegment> {
        self.segments.values()
    }

    /// How many segments it lists.
    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Whether it lists none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Whether stream directory `dir` holds exactly what this marker lists: the
    /// same segments at the same lengths, no temporary segment, and nothing in
    /// `journal`. If so, nothing has been written since. One `read_dir`, and one
    /// `stat` per segment; reads no segment.
    ///
    /// # Errors
    /// [`StorageError::Io`] when a directory cannot be listed. A missing journal
    /// directory is an empty one.
    pub fn is_current(&self, dir: &Path, journal: &Path) -> Result<bool> {
        let mut seen = 0;
        let entries = fs::read_dir(dir).map_err(|e| StorageError::io("list", dir, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| StorageError::io("list", dir, e))?;
            let path = entry.path();
            let Some(name) = SegmentName::parse(&path) else {
                if is_tmp(&path) {
                    return Ok(false);
                }
                continue;
            };
            let meta = entry
                .metadata()
                .map_err(|e| StorageError::io("stat", &path, e))?;
            if !self.vouches(name, meta.len()) {
                return Ok(false);
            }
            seen += 1;
        }
        Ok(seen == self.segments.len() && !holds_segments(journal)?)
    }
}

/// Whether `path` is an uncommitted segment.
fn is_tmp(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    name.is_some_and(|n| n.ends_with(&format!(".{SEGMENT_EXT}.tmp")))
}

fn holds_segments(dir: &Path) -> Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(StorageError::io("list", dir, e)),
    };
    for entry in entries {
        let path = entry.map_err(|e| StorageError::io("list", dir, e))?.path();
        if SegmentName::parse(&path).is_some() || is_tmp(&path) {
            return Ok(true);
        }
    }
    Ok(false)
}
