//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Appending a frame that was encoded elsewhere, so encoding can run in
//! parallel while a stream's writes stay sequential (SPEC §17).

use std::io::Write;

use super::SegmentWriter;
use crate::chunk::EncodedChunk;
use crate::error::{Bound, Result, StorageError};
use crate::format::frame::ChunkMeta;

impl SegmentWriter {
    /// Appends `chunk` as is and returns where it was written.
    ///
    /// # Errors
    /// [`StorageError::TooLarge`] when the directory is full (the segment is
    /// unchanged: roll first); [`StorageError::Sealed`] once poisoned;
    /// [`StorageError::Io`], which poisons the writer.
    pub fn append_encoded(&mut self, chunk: &EncodedChunk) -> Result<ChunkMeta> {
        if self.poisoned {
            return Err(StorageError::Sealed);
        }
        if self.chunks.len() >= self.opts.max_chunks_per_segment {
            return Err(StorageError::TooLarge {
                bound: Bound::SegmentChunks,
                len: self.chunks.len() as u64 + 1,
                max: self.opts.max_chunks_per_segment as u64,
            });
        }
        if let Err(e) = self.out.write_all(&chunk.frame) {
            self.poisoned = true;
            return Err(StorageError::io("write", &self.path, e));
        }
        let meta = ChunkMeta {
            offset: self.len,
            ..chunk.meta
        };
        self.len += chunk.frame.len() as u64;
        self.chunks.push(meta);
        self.range = Some(match self.range {
            None => (meta.min_ts, meta.max_ts),
            Some((a, b)) => (a.min(meta.min_ts), b.max(meta.max_ts)),
        });
        Ok(meta)
    }
}
