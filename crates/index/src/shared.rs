//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What every snapshot and the updater share: paths, bounds, cache, counters.

use std::path::{Path, PathBuf};

use norn_core::StreamId;
use norn_storage::{SegmentName, stream_dir};

use crate::cache::PageCache;
use crate::options::IndexOptions;
use crate::stats::Counters;

/// The directory under the database root holding every index file.
pub(crate) const INDEX_DIR: &str = "index";

/// Immutable configuration plus the two internally synchronised parts.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) root: PathBuf,
    pub(crate) options: IndexOptions,
    pub(crate) cache: PageCache,
    pub(crate) counters: Counters,
}

impl Shared {
    /// The index directory of `stream`.
    pub(crate) fn index_dir(&self, stream: StreamId) -> PathBuf {
        index_dir(&self.root, stream)
    }

    /// The segment file of `segment`.
    pub(crate) fn segment_path(&self, stream: StreamId, segment: SegmentName) -> PathBuf {
        stream_dir(&self.root, stream).join(segment.file_name())
    }

    /// The sidecar of `segment`.
    pub(crate) fn detail_path(&self, stream: StreamId, segment: SegmentName) -> PathBuf {
        self.index_dir(stream).join(detail_name(segment))
    }
}

/// `<root>/index/<stream, 16 hex digits>`.
pub(crate) fn index_dir(root: &Path, stream: StreamId) -> PathBuf {
    root.join(INDEX_DIR).join(format!("{:016x}", stream.get()))
}

/// A sidecar's file name.
pub(crate) fn detail_name(segment: SegmentName) -> String {
    format!(
        "{:016x}-g{:04x}.{}",
        segment.seq,
        segment.generation,
        crate::detail::DETAIL_EXT
    )
}
