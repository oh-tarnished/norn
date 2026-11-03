//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A bounded cache of open segment files.

use std::collections::VecDeque;

use norn_core::StreamId;
use norn_index::{ChunkLocator, Snapshot};
use norn_storage::{Chunk, ChunkFile, SegmentName, StorageError, StorageOptions};

use crate::memory::{Charge, Refused};

#[derive(Debug)]
struct Open {
    stream: StreamId,
    segment: SegmentName,
    sealed: bool,
    file: ChunkFile,
    /// The largest frame read, which is what this file's buffer holds.
    frame: u64,
}

/// Why a read through the cache failed.
#[derive(Debug)]
pub(crate) enum FileError {
    Memory(Refused),
    Storage(StorageError),
}

/// Open segments, least recently used first.
///
/// Locators of one segment are not adjacent in a plan, so this saves reopening
/// one for each chunk. A [`ChunkFile`] records the file's length when it opens,
/// so a file opened while unsealed is kept for one read only: [`Self::next_read`]
/// drops it, and the next snapshot's chunks are found in a fresh handle.
///
/// A handle on a segment compaction has since deleted still reads the old bytes,
/// which is what its snapshot wants.
///
/// Memory: `limit` handles, and each one's frame buffer, charged to `buffers`.
/// Concurrency: owned by one query.
#[derive(Debug)]
pub(crate) struct FileCache {
    open: VecDeque<Open>,
    limit: usize,
    opts: StorageOptions,
    buffers: Charge,
    retired_bytes: u64,
}

impl FileCache {
    pub(crate) fn new(limit: usize, opts: StorageOptions, buffers: Charge) -> Self {
        Self {
            open: VecDeque::new(),
            limit: limit.max(1),
            opts,
            buffers,
            retired_bytes: 0,
        }
    }

    /// Drops handles a newer snapshot could outrun.
    pub(crate) fn next_read(&mut self) {
        let (mut kept, mut retired) = (VecDeque::new(), 0);
        for entry in self.open.drain(..) {
            if entry.sealed {
                kept.push_back(entry);
            } else {
                retired += entry.file.bytes_read();
            }
        }
        self.open = kept;
        self.retired_bytes += retired;
        self.recharge();
    }

    fn recharge(&mut self) {
        let held: u64 = self.open.iter().map(|e| e.frame).sum();
        self.buffers.shrink_to(held);
    }

    /// Reads and verifies `locator`'s chunk. Copies.
    pub(crate) fn read(
        &mut self,
        snapshot: &Snapshot,
        locator: &ChunkLocator,
    ) -> Result<Chunk, FileError> {
        let same = |e: &Open| e.stream == locator.stream && e.segment == locator.segment;
        let found = self.open.iter().position(same);
        let mut entry = match found.and_then(|at| self.open.remove(at)) {
            Some(entry) => entry,
            None => {
                if self.open.len() >= self.limit
                    && let Some(old) = self.open.pop_front()
                {
                    self.retired_bytes += old.file.bytes_read();
                    self.recharge();
                }
                let file = ChunkFile::open(&snapshot.path_of(locator), self.opts);
                Open {
                    stream: locator.stream,
                    segment: locator.segment,
                    sealed: locator.sealed,
                    file: file.map_err(FileError::Storage)?,
                    frame: 0,
                }
            }
        };
        let frame = locator.chunk.frame_len();
        if frame > entry.frame {
            self.buffers
                .grow(frame - entry.frame)
                .map_err(FileError::Memory)?;
            entry.frame = frame;
        }
        let chunk = entry.file.read_chunk(&locator.chunk);
        self.open.push_back(entry);
        chunk.map_err(FileError::Storage)
    }

    /// Bytes read from segment files so far, by handles open or gone.
    pub(crate) fn bytes_read(&self) -> u64 {
        let open = self.open.iter().map(|e| e.file.bytes_read()).sum::<u64>();
        self.retired_bytes + open
    }
}
