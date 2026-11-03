//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Segments: one file of many chunks, written once, then sealed.
//!
//! A segment is append-only while open and immutable once sealed. Its file
//! name is `{first_seq:016x}-g{generation:04x}.nseg`, so a directory listing
//! sorts segments by sequence and the header can be cross-checked against
//! the name.

pub(crate) mod load;
mod located;
mod reader;
mod scan;
mod writer;

pub use located::ChunkFile;
pub use reader::SegmentReader;
pub use writer::SegmentWriter;

pub(crate) use scan::scan_headers;

use std::path::{Path, PathBuf};

use norn_core::Timestamp;

use crate::format::SegmentHeader;
use crate::format::frame::ChunkMeta;

/// File extension of a segment.
pub const SEGMENT_EXT: &str = "nseg";

/// A segment's identity as encoded in its file name.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegmentName {
    /// The first segment sequence number it covers.
    pub seq: u64,
    /// Its compaction generation.
    pub generation: u16,
}

impl SegmentName {
    /// The file name, e.g. `000000000000002a-g0000.nseg`.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{:016x}-g{:04x}.{SEGMENT_EXT}", self.seq, self.generation)
    }

    /// Parses a file name written by [`SegmentName::file_name`]; `None` for
    /// anything else (including `.tmp` files).
    #[must_use]
    pub fn parse(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let stem = name.strip_suffix(SEGMENT_EXT)?.strip_suffix('.')?;
        let (seq, generation) = stem.split_once("-g")?;
        if seq.len() != 16 || generation.len() != 4 {
            return None;
        }
        Some(Self {
            seq: u64::from_str_radix(seq, 16).ok()?,
            generation: u16::from_str_radix(generation, 16).ok()?,
        })
    }

    /// The name a segment with `header` must have.
    #[must_use]
    pub fn of(header: &SegmentHeader) -> Self {
        Self {
            seq: header.first_seq,
            generation: header.generation,
        }
    }
}

/// What a sealed or recovered segment holds, without its chunk list.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentSummary {
    /// The segment file.
    pub path: PathBuf,
    /// Its header.
    pub header: SegmentHeader,
    /// Number of chunks.
    pub chunks: usize,
    /// Number of samples over all chunks.
    pub samples: u64,
    /// Earliest and latest sample timestamp; `None` for an empty segment.
    pub time_range: Option<(Timestamp, Timestamp)>,
    /// File size in bytes, header, directory and trailer included.
    pub bytes: u64,
}

impl SegmentSummary {
    /// Summarises `chunks` of the segment at `path`.
    pub(crate) fn new(
        path: PathBuf,
        header: SegmentHeader,
        chunks: &[ChunkMeta],
        bytes: u64,
    ) -> Self {
        Self {
            path,
            header,
            chunks: chunks.len(),
            samples: chunks.iter().map(|c| u64::from(c.count)).sum(),
            time_range: time_range(chunks),
            bytes,
        }
    }
}

/// The earliest and latest timestamp over `chunks`.
pub(crate) fn time_range(chunks: &[ChunkMeta]) -> Option<(Timestamp, Timestamp)> {
    let min = chunks.iter().map(|c| c.min_ts).min()?;
    let max = chunks.iter().map(|c| c.max_ts).max()?;
    Some((min, max))
}
