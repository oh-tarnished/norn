//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Appending to a stream: a sequence of segments that roll by limits.

use std::path::{Path, PathBuf};

use norn_core::StreamId;

use crate::chunk::{Chunk, EncodedChunk};
use crate::error::{Bound, Result, StorageError};
use crate::format::SegmentHeader;
use crate::format::frame::ChunkMeta;
use crate::layout::list_segments;
use crate::lock::StreamLock;
use crate::options::StorageOptions;
use crate::platform::create_dir_durable;
use crate::segment::{SegmentSummary, SegmentWriter};

/// Where an appended chunk went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appended {
    /// The chunk's location and metadata.
    pub chunk: ChunkMeta,
    /// The segment it was written to.
    pub segment: PathBuf,
    /// The segment sealed to make room for it, if this append rolled one;
    /// the index layer learns of sealed segments from this.
    pub rolled: Option<SegmentSummary>,
}

/// Writes one stream's chunks into rolling segments, so no file grows
/// without bound (SPEC §13). A segment is sealed and the next one started
/// when it reaches `max_segment_bytes`, `max_chunks_per_segment` or
/// `max_segment_span_nanos` of sample time.
///
/// Lifecycle: [`StreamWriter::open`] creates nothing but the directory; the
/// first append creates a segment. Every new segment gets the next sequence
/// number after the highest one on disk. Run
/// [`crate::recover_stream`] first: an unsealed segment left by a crash is
/// sealed by recovery, never appended to.
///
/// Durability: as [`SegmentWriter`]; [`StreamWriter::sync`] covers the
/// current segment, and a rolled segment is sealed (and fsynced) before the
/// next one is created.
///
/// Memory: one [`SegmentWriter`]. Concurrency: `&mut self`; one writer per
/// stream directory, enforced by a [`StreamLock`] held until it is dropped.
#[derive(Debug)]
pub struct StreamWriter {
    dir: PathBuf,
    stream: StreamId,
    opts: StorageOptions,
    current: Option<SegmentWriter>,
    next_seq: u64,
    _lock: StreamLock,
}

impl StreamWriter {
    /// Opens stream `stream` in `dir`, creating the directory if missing.
    ///
    /// # Errors
    /// [`StorageError::Locked`] when another writer holds the stream;
    /// [`StorageError::Io`]; a segment header that does not verify (run
    /// recovery first); options that fail validation.
    pub fn open(dir: &Path, stream: StreamId, opts: StorageOptions) -> Result<Self> {
        opts.validate()?;
        create_dir_durable(dir)?;
        let lock = StreamLock::acquire(dir)?;
        let next_seq = list_segments(dir)?
            .iter()
            .map(|s| s.header.last_seq.saturating_add(1))
            .max()
            .unwrap_or(0);
        let dir = dir.to_path_buf();
        Ok(Self {
            dir,
            stream,
            opts,
            current: None,
            next_seq,
            _lock: lock,
        })
    }

    /// Appends `chunk`, rolling to a new segment first when the current one
    /// is full.
    ///
    /// # Errors
    /// As [`SegmentWriter::append`]; a chunk too large for an empty segment
    /// is refused with [`StorageError::TooLarge`].
    pub fn append(&mut self, chunk: &Chunk) -> Result<Appended> {
        let mut rolled = None;
        if self.current.as_ref().is_some_and(SegmentWriter::is_full) {
            rolled = self.roll()?;
        }
        let writer = self.writer()?;
        let meta = match writer.append(chunk) {
            Err(StorageError::TooLarge {
                bound: Bound::SegmentChunks,
                ..
            }) => {
                rolled = self.roll()?;
                self.writer()?.append(chunk)?
            }
            other => other?,
        };
        let segment = self.writer()?.path().to_path_buf();
        Ok(Appended {
            chunk: meta,
            segment,
            rolled,
        })
    }

    /// As [`StreamWriter::append`], for a chunk encoded ahead of time.
    ///
    /// # Errors
    /// As [`StreamWriter::append`].
    pub fn append_encoded(&mut self, chunk: &EncodedChunk) -> Result<Appended> {
        let mut rolled = None;
        if self.current.as_ref().is_some_and(SegmentWriter::is_full) {
            rolled = self.roll()?;
        }
        let meta = self.writer()?.append_encoded(chunk)?;
        let segment = self.writer()?.path().to_path_buf();
        Ok(Appended {
            chunk: meta,
            segment,
            rolled,
        })
    }

    /// Hands buffered bytes to the OS (survives a process crash).
    ///
    /// # Errors
    /// [`StorageError::Io`].
    pub fn flush(&mut self) -> Result<()> {
        self.current.as_mut().map_or(Ok(()), SegmentWriter::flush)
    }

    /// Makes every appended chunk durable (survives power loss).
    ///
    /// # Errors
    /// [`StorageError::Io`].
    pub fn sync(&mut self) -> Result<()> {
        self.current.as_mut().map_or(Ok(()), SegmentWriter::sync)
    }

    /// Seals the current segment, if any; the next append starts a new one.
    ///
    /// # Errors
    /// As [`SegmentWriter::seal`].
    pub fn roll(&mut self) -> Result<Option<SegmentSummary>> {
        self.current.take().map(SegmentWriter::seal).transpose()
    }

    /// Seals the current segment and closes the stream.
    ///
    /// # Errors
    /// As [`SegmentWriter::seal`].
    pub fn close(mut self) -> Result<Option<SegmentSummary>> {
        self.roll()
    }

    /// The segment appends are going to, if one is open.
    #[must_use]
    pub fn current_segment(&self) -> Option<&Path> {
        self.current.as_ref().map(SegmentWriter::path)
    }

    /// The stream directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn writer(&mut self) -> Result<&mut SegmentWriter> {
        if self.current.is_none() {
            let header = SegmentHeader::new(self.stream, self.next_seq);
            self.current = Some(SegmentWriter::create(&self.dir, header, self.opts)?);
            self.next_seq = self.next_seq.saturating_add(1);
        }
        self.current.as_mut().ok_or(StorageError::Sealed)
    }
}
