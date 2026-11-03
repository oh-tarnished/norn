//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Sealing a segment by age: maintenance asks, the store thread seals.
//!
//! A segment otherwise seals only when full, so a quiet or short-lived stream
//! kept everything in one open segment: compaction, retention and the clean-start
//! fast path all work on sealed segments, and passed it by. The store thread owns
//! the writer, so it is the one that seals; maintenance only decides when, on
//! the injected clock, so a test drives it with a fake one.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::time::Duration;

use norn_core::Timestamp;
use norn_storage::{StorageError, StreamWriter};

use super::Ingest;
use super::indexing::Publisher;
use super::shared::{NOT_OPEN, Shared};

/// The longest between two looks for segments to seal, so a long limit is
/// still kept to within a minute.
const MAX_CHECK: Duration = Duration::from_secs(60);
/// The shortest, so a tiny limit in a test cannot spin the worker.
const MIN_CHECK: Duration = Duration::from_millis(10);

/// What the store thread remembers of its open segment. Owned by that thread.
#[derive(Debug, Default)]
pub(crate) struct OpenSegment {
    path: Option<PathBuf>,
}

impl OpenSegment {
    /// After a store: when the writer has moved to a new segment, records that
    /// it opened at `now`. Reads nothing but the writer.
    pub(crate) fn note(&mut self, writer: &StreamWriter, shared: &Shared, now: Timestamp) {
        let current = writer.current_segment();
        if current == self.path.as_deref() {
            return;
        }
        self.path = current.map(Path::to_path_buf);
        let at = current.map_or(NOT_OPEN, |_| now.as_nanos());
        shared.opened_at.store(at, Ordering::Release);
    }

    /// Seals the open segment if maintenance asked, and tells the index. Called
    /// between stores only, so every chunk written is already fsynced and
    /// published. A request with no segment open is simply cleared.
    ///
    /// # Errors
    /// As [`StreamWriter::roll`]; the caller fails the stream, and recovery
    /// seals the segment at the next start.
    pub(crate) fn seal_if_asked(
        &mut self,
        writer: &mut StreamWriter,
        shared: &Shared,
        publisher: &mut Publisher<'_>,
    ) -> Result<(), StorageError> {
        if !shared.seal_requested.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        if let Some(summary) = writer.roll()? {
            tracing::debug!(stream = shared.stream.get(), "segment sealed by age");
            publisher.sealed(&summary);
        }
        self.path = None;
        shared.opened_at.store(NOT_OPEN, Ordering::Release);
        Ok(())
    }
}

impl Ingest {
    /// Asks every stream whose segment has been open for
    /// `storage.max_segment_open_time` or longer at `now` to seal it, and
    /// returns how many were asked. The store thread seals within a second, or
    /// after the store it is in. Never blocks on the disk; one short lock.
    pub fn seal_aged(&self, now: Timestamp) -> usize {
        let limit = i64::try_from(self.ctx.max_open.as_nanos()).unwrap_or(i64::MAX);
        let streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        let mut asked = 0;
        for handle in streams.values() {
            let opened = handle.shared.opened_at.load(Ordering::Acquire);
            if opened == NOT_OPEN {
                continue;
            }
            if now.as_nanos().saturating_sub(opened) >= limit {
                handle.shared.seal_requested.store(true, Ordering::Release);
                asked += 1;
            }
        }
        asked
    }

    /// How often maintenance should call [`seal_aged`](Self::seal_aged): a
    /// quarter of the limit, between 10 ms and a minute, so a segment is sealed
    /// at most a quarter late.
    #[must_use]
    pub fn seal_check_interval(&self) -> Duration {
        (self.ctx.max_open / 4).clamp(MIN_CHECK, MAX_CHECK)
    }
}
