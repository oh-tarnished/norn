//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading chunks whose location is already known, without the directory.

use std::path::Path;

use super::load::{read_frame, read_header};
use crate::chunk::Chunk;
use crate::chunk::decode::decode_body;
use crate::crypt::{SegmentCipher, cipher_for};
use crate::error::Result;
use crate::format::frame::ChunkMeta;
use crate::format::{FRAME_HEADER_LEN, SegmentHeader};
use crate::fsio::Source;
use crate::options::StorageOptions;

/// A segment opened for reads at known offsets.
///
/// [`crate::SegmentReader::open`] reads the whole directory, 704 KB for a
/// compacted hour; a caller holding [`ChunkMeta`]s from the temporal index
/// needs none of it. Opening reads the 48-byte header, and each
/// [`ChunkFile::read_chunk`] exactly one frame.
///
/// It works on sealed and unsealed segments alike, for bytes that had reached
/// the OS when it was opened: the file length is recorded then, so a chunk
/// flushed later needs a new `ChunkFile`.
///
/// Failure: every read checks the frame header against `meta` and the body
/// against its CRC, so a stale `meta` is a typed
/// [`crate::StorageError::Corrupt`] with `DirectoryMismatch`, never wrong data.
/// Memory: one frame buffer, reused. Concurrency: reads take `&mut self`; open
/// one per thread.
#[derive(Debug)]
pub struct ChunkFile {
    src: Source,
    header: SegmentHeader,
    cipher: Option<SegmentCipher>,
    opts: StorageOptions,
    buf: Vec<u8>,
}

impl ChunkFile {
    /// Opens the segment at `path`, reading and verifying its header.
    ///
    /// # Errors
    /// [`crate::StorageError::Io`], with `NotFound` when compaction or
    /// retention has removed the file; [`crate::StorageError::Corrupt`] for a
    /// bad header; [`crate::StorageError::UnsupportedVersion`].
    pub fn open(path: &Path, opts: StorageOptions) -> Result<Self> {
        let mut src = Source::open(path)?;
        let header = read_header(&mut src)?;
        let cipher = cipher_for(&header, &opts, path)?;
        Ok(Self {
            src,
            header,
            cipher,
            opts,
            buf: Vec::new(),
        })
    }

    /// Reads, verifies and decodes the chunk `meta` describes. Copies out of
    /// the internal buffer; the result borrows nothing.
    ///
    /// # Errors
    /// As [`crate::SegmentReader::read_chunk`].
    pub fn read_chunk(&mut self, meta: &ChunkMeta) -> Result<Chunk> {
        let cipher = self.cipher.as_ref();
        read_frame(&mut self.src, meta, &self.opts, cipher, &mut self.buf)?;
        let body = self.buf.get(FRAME_HEADER_LEN..).unwrap_or_default();
        decode_body(meta, body, self.src.path())
    }

    /// The segment header.
    #[must_use]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }

    /// Bytes read from the file since it was opened, header included.
    #[must_use]
    pub fn bytes_read(&self) -> u64 {
        self.src.bytes_read()
    }
}
