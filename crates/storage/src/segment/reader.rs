//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Opening a segment, listing its chunks, and reading chosen ones.

use std::path::Path;

use norn_core::{SignalId, Timestamp};

use super::load::{load_directory, read_frame, read_header};
use super::{SegmentSummary, scan_headers};
use crate::chunk::Chunk;
use crate::chunk::decode::decode_body;
use crate::chunk::layout::{ChunkLayout, layout_body};
use crate::crypt::{SegmentCipher, cipher_for};
use crate::error::Result;
use crate::format::frame::ChunkMeta;
use crate::format::{FRAME_HEADER_LEN, SegmentHeader};
use crate::fsio::Source;
use crate::iter::SampleIter;
use crate::options::StorageOptions;

/// A segment opened for reading.
///
/// Opening reads the header and the directory (sealed) or walks the frame
/// headers (unsealed); it decodes no chunk. Reads are then partial: a query
/// for one signal over one hour reads that signal's frames and nothing
/// else ([`SegmentReader::bytes_read`] proves it in the tests).
///
/// Failure: a corrupt chunk fails only its own read, with a typed
/// [`crate::StorageError::Corrupt`]; the others stay readable.
/// Memory: the directory, plus one frame buffer reused across reads.
/// Concurrency: reads take `&mut self` (one file cursor); open one reader
/// per thread for parallel reads. A sealed segment never changes.
#[derive(Debug)]
pub struct SegmentReader {
    src: Source,
    header: SegmentHeader,
    cipher: Option<SegmentCipher>,
    chunks: Vec<ChunkMeta>,
    sealed: bool,
    tail: Option<(u64, u64)>,
    opts: StorageOptions,
    buf: Vec<u8>,
}

impl SegmentReader {
    /// Opens the segment at `path`.
    ///
    /// # Errors
    /// [`crate::StorageError::Io`]; [`crate::StorageError::Corrupt`] for a bad header or
    /// a seal marker whose directory is corrupt (run recovery);
    /// [`crate::StorageError::UnsupportedVersion`]; [`crate::StorageError::TooLarge`] for
    /// a directory above `max_chunks_per_segment`.
    pub fn open(path: &Path, opts: StorageOptions) -> Result<Self> {
        let mut src = Source::open(path)?;
        let header = read_header(&mut src)?;
        let cipher = cipher_for(&header, &opts, path)?;
        let (chunks, sealed, tail) = match load_directory(&mut src, &opts)? {
            Some(chunks) => (chunks, true, None),
            None => {
                let end = src.len();
                let (chunks, stop) = scan_headers(&mut src, header.data_start(), end, &opts)?;
                let tail = (stop < src.len()).then(|| (stop, src.len() - stop));
                (chunks, false, tail)
            }
        };
        let buf = Vec::new();
        Ok(Self {
            src,
            header,
            cipher,
            chunks,
            sealed,
            tail,
            opts,
            buf,
        })
    }

    /// Every chunk, in file order.
    #[must_use]
    pub fn chunks(&self) -> &[ChunkMeta] {
        &self.chunks
    }

    /// The chunks of `signal` (any signal when `None`) that may hold
    /// samples in `from..=to`, in file order. Reads nothing.
    pub fn select(
        &self,
        signal: Option<SignalId>,
        from: Timestamp,
        to: Timestamp,
    ) -> impl Iterator<Item = &ChunkMeta> + '_ {
        self.chunks
            .iter()
            .filter(move |c| signal.is_none_or(|s| s == c.signal) && c.overlaps(from, to))
    }

    /// Reads, verifies and decodes one chunk.
    ///
    /// Cost: one read of the frame; decoding is O(samples).
    ///
    /// # Errors
    /// [`crate::StorageError::Corrupt`] for a frame header that disagrees with
    /// `meta`, a body CRC mismatch, or a failed authentication;
    /// [`crate::StorageError::Codec`];
    /// [`crate::StorageError::TooLarge`]; [`crate::StorageError::Io`].
    pub fn read_chunk(&mut self, meta: &ChunkMeta) -> Result<Chunk> {
        let cipher = self.cipher.as_ref();
        read_frame(&mut self.src, meta, &self.opts, cipher, &mut self.buf)?;
        let body = self.buf.get(FRAME_HEADER_LEN..).unwrap_or_default();
        decode_body(meta, body, self.src.path())
    }

    /// Reads, verifies and decodes one chunk, and reports how each of its
    /// columns is stored, for inspection tools. Keeps no sample.
    ///
    /// Cost: as [`SegmentReader::read_chunk`].
    ///
    /// # Errors
    /// As [`SegmentReader::read_chunk`].
    pub fn read_layout(&mut self, meta: &ChunkMeta) -> Result<ChunkLayout> {
        let cipher = self.cipher.as_ref();
        read_frame(&mut self.src, meta, &self.opts, cipher, &mut self.buf)?;
        let body = self.buf.get(FRAME_HEADER_LEN..).unwrap_or_default();
        layout_body(meta, body, self.src.path())
    }

    /// Streams the samples of `signal` in `from..=to`, one decoded chunk at
    /// a time (see [`SampleIter`]).
    pub fn samples(
        &mut self,
        signal: Option<SignalId>,
        from: Timestamp,
        to: Timestamp,
    ) -> SampleIter<'_> {
        let metas = self.select(signal, from, to).copied().collect();
        SampleIter::new(self, metas, from, to)
    }

    /// The segment header.
    #[must_use]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }

    /// Whether the segment carries a valid seal (directory and trailer).
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// For an unsealed segment, the `(offset, len)` of bytes after the last
    /// complete frame header: an in-progress or torn write. Recovery decides
    /// what it is.
    #[must_use]
    pub fn unverified_tail(&self) -> Option<(u64, u64)> {
        self.tail
    }

    /// Bytes read from the file since it was opened, header and directory
    /// included.
    #[must_use]
    pub fn bytes_read(&self) -> u64 {
        self.src.bytes_read()
    }

    /// What the segment holds, from its directory; reads nothing.
    #[must_use]
    pub fn summary(&self) -> SegmentSummary {
        let path = self.src.path().to_path_buf();
        SegmentSummary::new(path, self.header, &self.chunks, self.src.len())
    }

    /// The segment file.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.src.path()
    }
}
