//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How a stream's history sits on disk, for inspection: `GetSegment` and
//! `ListSegments` (docs/reference/storage.md, "Segments").
//!
//! Listing reads the temporal index and stats open segments' files; describing
//! one segment in full reads every chunk of it. Nothing here writes, and
//! nothing waits for the maintenance worker: a segment compaction or retention
//! removes mid-read is reported as gone ([`LayoutError::NotFound`]).

use std::collections::BTreeMap;
use std::path::Path;

use norn_core::{StreamId, Timestamp};
use norn_storage::{ChunkMeta, SegmentName, SegmentReader, StorageError, stream_dir};

use super::Ingest;
use super::layout_detail::detail;
pub use super::layout_detail::{ChunkColumn, ColumnEncoding, SegmentDetail};

/// What the index knows of one segment file.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SegmentInfo {
    /// The file name without its extension, such as `000000000000002a-g0000`.
    pub id: String,
    /// Whether it is sealed; an open one still grows.
    pub sealed: bool,
    /// Its compaction generation.
    pub generation: u16,
    /// Chunks in it.
    pub chunks: u64,
    /// Samples in those chunks.
    pub samples: u64,
    /// The file's length; for an open segment, as of the listing.
    pub size_bytes: u64,
    /// The earliest and latest sample time; `None` for no chunk.
    pub time_range: Option<(Timestamp, Timestamp)>,
}

/// Why a segment could not be described.
///
/// Concurrency: `Send + Sync`.
#[derive(Debug)]
#[non_exhaustive]
pub enum LayoutError {
    /// No such segment: never written, or removed by compaction or retention.
    NotFound,
    /// The file could not be read at all.
    Storage(StorageError),
}

/// The segment named `id`, parsed; `None` for anything Norn never writes.
fn parse(id: &str) -> Option<SegmentName> {
    SegmentName::parse(Path::new(&format!("{id}.{}", norn_storage::SEGMENT_EXT)))
}

fn stem(name: SegmentName) -> String {
    let file = name.file_name();
    let ext = format!(".{}", norn_storage::SEGMENT_EXT);
    file.strip_suffix(&ext).unwrap_or(&file).to_owned()
}

fn widen(range: &mut Option<(Timestamp, Timestamp)>, from: Timestamp, to: Timestamp) {
    *range = Some(range.map_or((from, to), |(a, b)| (a.min(from), b.max(to))));
}

impl Ingest {
    /// Every segment of `stream`, sealed and open, in name order. Reads the
    /// index; stats each open segment's file for its length.
    ///
    /// Cost: one entry per segment, and 64 bytes per open chunk while listing.
    #[must_use]
    pub fn segments(&self, stream: StreamId) -> Vec<SegmentInfo> {
        let Some(index) = self.index() else {
            return Vec::new();
        };
        let snapshot = index.snapshot();
        let mut out: BTreeMap<SegmentName, SegmentInfo> = BTreeMap::new();
        for r in snapshot.segments(stream) {
            out.insert(
                r.name(),
                SegmentInfo {
                    id: stem(r.name()),
                    sealed: true,
                    generation: r.generation,
                    chunks: u64::from(r.chunks),
                    samples: r.samples,
                    size_bytes: r.file_len,
                    time_range: (r.chunks > 0).then_some((r.min_ts, r.max_ts)),
                },
            );
        }
        let dir = stream_dir(self.root(), stream);
        for l in snapshot.tail_locators(stream) {
            let entry = out.entry(l.segment).or_insert_with(|| SegmentInfo {
                id: stem(l.segment),
                sealed: false,
                generation: l.segment.generation,
                chunks: 0,
                samples: 0,
                size_bytes: std::fs::metadata(dir.join(l.segment.file_name()))
                    .map_or(0, |m| m.len()),
                time_range: None,
            });
            if entry.sealed {
                continue;
            }
            entry.chunks += 1;
            entry.samples += u64::from(l.chunk.count);
            widen(&mut entry.time_range, l.chunk.min_ts, l.chunk.max_ts);
        }
        out.into_values().collect()
    }

    /// The segment `id` of `stream`, read whole: every chunk is read, checked
    /// and decoded, one at a time, for its encodings. Blocking; call it off
    /// the async executor.
    ///
    /// Cost: the whole file, one frame in memory at a time.
    ///
    /// # Errors
    /// [`LayoutError::NotFound`] for a name the index does not list, or a file
    /// gone before it could be read; [`LayoutError::Storage`] when it cannot be
    /// opened or read. A damaged chunk is counted, not an error.
    pub fn segment_detail(
        &self,
        stream: StreamId,
        id: &str,
    ) -> Result<(SegmentInfo, SegmentDetail), LayoutError> {
        let name = parse(id).ok_or(LayoutError::NotFound)?;
        let info = self.segments(stream).into_iter().find(|s| s.id == id);
        let mut info = info.ok_or(LayoutError::NotFound)?;
        let path = stream_dir(self.root(), stream).join(name.file_name());
        let mut reader = SegmentReader::open(&path, self.storage_options()).map_err(gone)?;
        let chunks: Vec<ChunkMeta> = reader.chunks().to_vec();
        let found = detail(&mut reader, &chunks).map_err(gone)?;
        if !info.sealed {
            info.chunks = chunks.len() as u64;
            info.samples = chunks.iter().map(|c| u64::from(c.count)).sum();
            info.size_bytes = std::fs::metadata(&path).map_or(info.size_bytes, |m| m.len());
            info.time_range = None;
            for c in &chunks {
                widen(&mut info.time_range, c.min_ts, c.max_ts);
            }
        }
        Ok((info, found))
    }
}

fn gone(e: StorageError) -> LayoutError {
    match e {
        StorageError::Io { ref source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            LayoutError::NotFound
        }
        other => LayoutError::Storage(other),
    }
}
