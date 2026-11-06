//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Appending chunks to a segment; `durability` holds flush, sync and seal.

mod durability;
mod encoded;

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use norn_codec::compress::Compression;
use norn_core::Timestamp;

use super::SegmentName;
use crate::chunk::Chunk;
use crate::chunk::encode::encode_frame;
use crate::crypt::SegmentCipher;
use crate::error::{Bound, Result, StorageError};
use crate::format::SegmentHeader;
use crate::format::frame::ChunkMeta;
use crate::options::StorageOptions;

/// Writes one segment: header, then chunk frames in order, then the seal.
///
/// Durability is explicit, so the write path can map it onto SPEC §19:
/// [`SegmentWriter::append`] buffers (up to `write_buffer_bytes`),
/// [`SegmentWriter::flush`] hands the bytes to the OS (they survive a
/// process crash, not a power loss), and [`SegmentWriter::sync`] fsyncs the
/// file, and its directory the first time, so they survive power loss.
///
/// Failure: if a write fails part-way, the writer is poisoned and refuses
/// further appends; the file may end in a torn frame, which recovery
/// detects, quarantines and truncates.
///
/// Memory: the buffer, one encoded frame, and the directory
/// (`DIR_ENTRY_LEN` bytes a chunk, at most `max_chunks_per_segment`).
/// Concurrency: `&mut self` throughout; one writer per segment file, and a
/// segment is never opened for writing twice (creation is exclusive).
#[derive(Debug)]
pub struct SegmentWriter {
    out: BufWriter<File>,
    path: PathBuf,
    header: SegmentHeader,
    opts: StorageOptions,
    cipher: Option<SegmentCipher>,
    len: u64,
    chunks: Vec<ChunkMeta>,
    range: Option<(Timestamp, Timestamp)>,
    frame: Vec<u8>,
    dir_synced: bool,
    poisoned: bool,
}

impl SegmentWriter {
    /// Creates segment `header` in `dir`, named by [`SegmentName::of`].
    ///
    /// # Errors
    /// [`StorageError::Io`] if the file already exists or cannot be
    /// written; options that fail [`StorageOptions::validate`].
    pub fn create(dir: &Path, header: SegmentHeader, opts: StorageOptions) -> Result<Self> {
        Self::create_at(dir.join(SegmentName::of(&header).file_name()), header, opts)
    }

    /// Creates a segment at exactly `path` (compaction writes to a `.tmp`
    /// name and renames once sealed). Exclusive: never overwrites.
    ///
    /// # Errors
    /// As [`SegmentWriter::create`].
    pub fn create_at(
        path: PathBuf,
        mut header: SegmentHeader,
        opts: StorageOptions,
    ) -> Result<Self> {
        opts.validate()?;
        let cipher = SegmentCipher::for_new(&mut header, &opts)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| StorageError::io("create", &path, e))?;
        let mut out = BufWriter::with_capacity(opts.write_buffer_bytes, file);
        out.write_all(&header.encode_all())
            .map_err(|e| StorageError::io("write", &path, e))?;
        Ok(Self {
            out,
            path,
            header,
            opts,
            cipher,
            len: header.data_start(),
            chunks: Vec::new(),
            range: None,
            frame: Vec::new(),
            dir_synced: false,
            poisoned: false,
        })
    }

    /// Appends `chunk` with the configured compression.
    ///
    /// # Errors
    /// See [`SegmentWriter::append_with`].
    pub fn append(&mut self, chunk: &Chunk) -> Result<ChunkMeta> {
        self.append_with(chunk, self.opts.compression)
    }

    /// Appends `chunk`, compressing its columns with `how`, and returns
    /// where it was written. Nothing is durable until [`SegmentWriter::sync`].
    ///
    /// # Errors
    /// [`StorageError::TooLarge`] when the directory is full or the chunk
    /// exceeds a bound (the segment is unchanged: roll first);
    /// [`StorageError::InvalidChunk`] for an empty chunk;
    /// [`StorageError::Sealed`] once poisoned; [`StorageError::Io`].
    pub fn append_with(&mut self, chunk: &Chunk, how: Compression) -> Result<ChunkMeta> {
        if self.poisoned {
            return Err(StorageError::Sealed);
        }
        if self.chunks.len() >= self.opts.max_chunks_per_segment {
            let (len, max) = (
                self.chunks.len() as u64 + 1,
                self.opts.max_chunks_per_segment as u64,
            );
            return Err(StorageError::TooLarge {
                bound: Bound::SegmentChunks,
                len,
                max,
            });
        }
        self.frame.clear();
        let mut meta = encode_frame(chunk, how, &self.opts, self.len, &mut self.frame)?;
        if let Some(cipher) = &self.cipher {
            meta = cipher.seal_frame(&mut self.frame, 0, &meta, self.len)?;
        }
        if let Err(e) = self.out.write_all(&self.frame) {
            self.poisoned = true;
            return Err(StorageError::io("write", &self.path, e));
        }
        self.len += self.frame.len() as u64;
        self.chunks.push(meta);
        self.range = Some(match self.range {
            None => (meta.min_ts, meta.max_ts),
            Some((a, b)) => (a.min(meta.min_ts), b.max(meta.max_ts)),
        });
        Ok(meta)
    }

    /// Whether the segment has reached a roll limit: size, chunk count, or
    /// the span of its sample timestamps. The caller seals it and starts
    /// the next one.
    #[must_use]
    pub fn is_full(&self) -> bool {
        let span = self
            .range
            .map_or(0, |(a, b)| b.nanos_since(a).unwrap_or(i64::MAX));
        self.len >= self.opts.max_segment_bytes
            || self.chunks.len() >= self.opts.max_chunks_per_segment
            || span >= self.opts.max_segment_span_nanos
    }

    /// Bytes written so far, buffered ones included.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether no chunk has been appended.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The chunks appended so far.
    #[must_use]
    pub fn chunks(&self) -> &[ChunkMeta] {
        &self.chunks
    }

    /// The segment file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The segment header.
    #[must_use]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }
}
