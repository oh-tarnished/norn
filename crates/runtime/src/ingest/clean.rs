//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The clean-shutdown marker's two ends: writing it after a close, and a start
//! that needs no more than it (docs/reference/storage.md, "Clean shutdown").

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use norn_core::{SignalId, StreamId, Timestamp};
use norn_index::{SignalSpan, Snapshot};
use norn_storage::{
    ChunkFile, CleanMarker, CleanSegment, SegmentName, StorageError, live_segments, stream_dir,
};

use super::Ingest;
use super::dedup::Dedup;
use super::journal::journal_dir;
use super::report::IngestRecovery;
use super::stats::StreamStats;

impl Ingest {
    /// The stream's marker, if it says nothing has been written since: then its
    /// totals stand in for reading it, and the stream joins `clean`.
    ///
    /// # Errors
    /// [`StorageError`] when a directory cannot be listed.
    pub(super) fn clean_start(
        &self,
        id: StreamId,
        dir: &Path,
        total: &mut IngestRecovery,
        clean: &mut Vec<(StreamId, StreamStats)>,
    ) -> Result<bool, StorageError> {
        let Some(marker) = CleanMarker::load(dir, id) else {
            return Ok(false);
        };
        if !marker.is_current(dir, &journal_dir(dir))? {
            return Ok(false);
        }
        let mut stats = StreamStats::default();
        for s in marker.segments() {
            stats.add(s.samples, s.bytes, s.time_range);
        }
        total.clean_streams += 1;
        total.vouched_segments += marker.len() as u64;
        clean.push((id, stats));
        Ok(true)
    }

    /// Rebuilds what admission needs for the streams of `clean`: each signal's
    /// high-water mark from its latest chunk, through the index, then the window
    /// from the chunks written last ([`super::scan::seed_recent`]). Costs a page
    /// and a chunk per signal, and at most a window of samples, whatever the history. A chunk that
    /// fails to read is left for a query or `VerifyStream` to report.
    pub(super) fn seed_clean(&self, clean: Vec<(StreamId, StreamStats)>) {
        let Some(index) = self.ctx.index.get() else {
            return;
        };
        let snapshot = index.snapshot();
        for (id, stats) in clean {
            let mut dedup = Dedup::new(&self.ctx.budget);
            self.seed_stream(&snapshot, id, &mut dedup);
            let mut map = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
            map.insert(id, stats);
            drop(map);
            let mut held = self
                .recovered
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            held.insert(id, (dedup, stats));
        }
    }

    fn seed_stream(&self, snapshot: &Arc<Snapshot>, id: StreamId, dedup: &mut Dedup) {
        let mut signals: Vec<(SignalId, SignalSpan)> = snapshot.signals(id).collect();
        signals.sort_by_key(|(_, span)| span.last_segment);
        let mut open: Option<(SegmentName, ChunkFile)> = None;
        for (signal, span) in signals {
            self.ctx.registry.mark_stored(signal);
            let Ok(Some(locator)) = snapshot.nearest_at_or_before(signal, span.last) else {
                continue;
            };
            if open
                .as_ref()
                .is_none_or(|(name, _)| *name != locator.segment)
            {
                let file = ChunkFile::open(&snapshot.path_of(&locator), self.ctx.opts);
                open = file.ok().map(|f| (locator.segment, f));
            }
            let Some((_, file)) = open.as_mut() else {
                continue;
            };
            if let Ok(chunk) = file.read_chunk(&locator.chunk) {
                for sample in chunk.samples() {
                    dedup.admit(&sample);
                }
            }
        }
        if let Ok(segments) = live_segments(&stream_dir(&self.ctx.root, id)) {
            super::scan::seed_recent(&segments, &self.ctx.opts, dedup);
        }
    }

    /// After every thread has joined: writes each stream's marker, listing what
    /// the index holds, or removes it if the stream cannot be vouched for: a
    /// failure, known damage, samples not stored, or a directory that does not
    /// hold exactly what the index does. Failure here costs the next start time.
    pub(super) fn write_clean_markers(&self) {
        let Some(index) = self.ctx.index.get() else {
            return;
        };
        let snapshot = index.snapshot();
        for id in self.ctx.registry.view().stream_ids() {
            let dir = stream_dir(&self.ctx.root, id);
            if !dir.exists() {
                continue;
            }
            let result = if self.vouchable(&snapshot, id) {
                let records = snapshot.segments(id).iter().map(|r| CleanSegment {
                    name: r.name(),
                    bytes: r.file_len,
                    samples: r.samples,
                    time_range: (r.chunks > 0).then_some((r.min_ts, r.max_ts)),
                });
                let marker = CleanMarker::new(id, records);
                match marker.is_current(&dir, &journal_dir(&dir)) {
                    Ok(true) => marker.save(&dir),
                    Ok(false) => CleanMarker::remove(&dir),
                    Err(e) => Err(e),
                }
            } else {
                CleanMarker::remove(&dir)
            };
            if let Err(e) = result {
                tracing::warn!(stream = id.get(), error = %e, "clean marker not written");
            }
        }
    }

    fn vouchable(&self, snapshot: &Snapshot, id: StreamId) -> bool {
        let stats = self.stats(id);
        let troubled = self.running(id).is_some_and(|s| {
            s.failed.load(Ordering::Acquire) || s.index_behind.load(Ordering::Acquire)
        });
        !troubled
            && stats.corrupt_samples == 0
            && stats.buffered == 0
            && snapshot.tail_chunks(id) == 0
    }

    /// The earliest sample time among `stream`'s sealed segments.
    pub(crate) fn oldest_stored(&self, stream: StreamId) -> Option<Timestamp> {
        let snapshot = self.ctx.index.get()?.snapshot();
        let records = snapshot.segments(stream).iter();
        records.filter(|r| r.chunks > 0).map(|r| r.min_ts).min()
    }
}
