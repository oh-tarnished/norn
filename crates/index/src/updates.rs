//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the write path and compaction tell the index.
//!
//! Each call builds a new view outside any reader's way and swaps it in, then
//! persists. Crash at any step leaves a stale or absent file, which startup
//! reconciles. All are idempotent, and all but `publish_chunks` touch disk.

use std::collections::HashMap;
use std::sync::{Arc, MutexGuard, PoisonError};

use norn_core::StreamId;
use norn_storage::format::HEADER_LEN;
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{CompactionOutcome, SegmentName, SegmentSummary, stream_dir};

use crate::build::{Scanned, scan_all, scan_segment};
use crate::error::Result;
use crate::index::TemporalIndex;
use crate::reconcile::{list_files, persist, remove_orphans};
use crate::report::{StreamReport, SummaryState};
use crate::signals::{SignalMap, absorb_record, absorb_tail, rebuild};
use crate::tail::TailLog;
use crate::view::StreamView;

impl TemporalIndex {
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.updates.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn view(&self, stream: StreamId) -> Arc<StreamView> {
        let snapshot = self.snapshot();
        let view = snapshot.streams.get(&stream).map(Arc::clone);
        view.unwrap_or_else(|| Arc::new(StreamView::new(stream, Vec::new(), Vec::new())))
    }

    /// Makes `metas` of the unsealed `segment` visible to new snapshots.
    ///
    /// Call after `StreamWriter::flush` or `sync`, never before: a locator past
    /// the end of the file would be unreadable. `metas` are in file order, and
    /// metas already published or for a segment already sealed are ignored, so a
    /// retry is harmless.
    ///
    /// Cost: copies at most one 256-entry block, and touches no disk.
    ///
    /// # Errors
    /// [`crate::IndexError::OverBudget`]: nothing changed; seal to reclaim room.
    pub fn publish_chunks(
        &self,
        stream: StreamId,
        segment: SegmentName,
        metas: &[ChunkMeta],
    ) -> Result<()> {
        let _guard = self.lock();
        let view = self.view(stream);
        if view.covers(segment) {
            return Ok(());
        }
        let tail = view.tails.iter().find(|t| t.segment == segment);
        let next = tail.map_or_else(
            || TailLog::new(segment).appended(metas),
            |t| t.appended(metas),
        );
        let signals = view.signals.absorbed(segment, metas);
        self.install(stream, view.with_tail(next).with_signals(Arc::new(signals)))
    }

    /// Records that a segment sealed: `Appended::rolled`, `StreamWriter::roll` or
    /// `close`. Reads its directory once, writes its sidecar, then swaps the
    /// tail for the sealed record in one step, so no chunk is missing or
    /// answered twice.
    ///
    /// # Errors
    /// [`crate::IndexError::Storage`]; [`crate::IndexError::WrongStream`];
    /// [`crate::IndexError::OverBudget`];
    /// [`crate::IndexError::PersistFailed`], after the update took effect.
    pub fn segment_sealed(&self, stream: StreamId, summary: &SegmentSummary) -> Result<()> {
        let _guard = self.lock();
        let Scanned::Sealed(record) = scan_segment(&self.shared, stream, &summary.path)? else {
            return Ok(());
        };
        self.apply_record(stream, record)
    }

    /// Records a finished compaction: the output replaces its inputs in one step,
    /// and readers of older snapshots follow [`TemporalIndex`]'s rules.
    ///
    /// # Errors
    /// As [`TemporalIndex::segment_sealed`].
    pub fn compaction_applied(&self, stream: StreamId, outcome: &CompactionOutcome) -> Result<()> {
        let _guard = self.lock();
        let path = &outcome.output.path;
        let Scanned::Sealed(record) = scan_segment(&self.shared, stream, path)? else {
            return Ok(());
        };
        // The inputs go by the liveness rule: the output supersedes them.
        self.apply_record(stream, record)
    }

    fn apply_record(&self, stream: StreamId, record: crate::record::SegmentRecord) -> Result<()> {
        let view = self.view(stream).with_record(record);
        let signals = absorb_record(&self.shared, &view, &view.signals, &record)?;
        self.finish(stream, view.with_signals(Arc::new(signals)))
    }

    /// Reconciles `stream` with its directory. The catch-all: after a reader met a
    /// missing file, after retention deleted segments, or when anything is in
    /// doubt. Reads only what it does not know. The tail of a file still present
    /// is kept, since only the writer knows it has moved on.
    ///
    /// A file it does not know that is shorter than a segment header is skipped:
    /// a writer has created it and its header is still in the writer's buffer, so
    /// there is nothing to read yet, and that writer publishes it. Runs beside a
    /// writer, since a reader racing a compaction calls it at any moment.
    ///
    /// # Errors
    /// [`crate::IndexError::Io`]; [`crate::IndexError::Storage`];
    /// [`crate::IndexError::OverBudget`];
    /// [`crate::IndexError::PersistFailed`], after the update took effect.
    pub fn refresh(&self, stream: StreamId) -> Result<StreamReport> {
        let _guard = self.lock();
        let view = self.view(stream);
        let listed = list_files(&stream_dir(&self.shared.root, stream))?;
        let lens: HashMap<_, _> = listed.iter().map(|l| (l.name, l.len)).collect();
        let known = view.segments.records.iter().map(|r| (r.name(), r));
        let known: HashMap<_, _> = known.collect();
        let tails = view.tails.iter().filter(|t| lens.contains_key(&t.segment));
        let mut tails: Vec<_> = tails.cloned().collect();
        let mut records = Vec::with_capacity(listed.len());
        let mut unknown = Vec::new();
        for file in &listed {
            match known.get(&file.name) {
                Some(r) if r.file_len == file.len => records.push(**r),
                _ if tails.iter().any(|t| t.segment == file.name) => {}
                _ if file.len < HEADER_LEN as u64 => {}
                _ => unknown.push(file.path.clone()),
            }
        }
        let reused = records.len();
        let (mut fresh, mut fresh_tails) = (Vec::new(), Vec::new());
        for scanned in scan_all(&self.shared, stream, &unknown)? {
            match scanned {
                Scanned::Sealed(r) => {
                    fresh.push(r);
                    records.push(r);
                }
                Scanned::Unsealed(t) => {
                    let t = Arc::new(t);
                    fresh_tails.push(Arc::clone(&t));
                    tails.push(t);
                }
            }
        }
        let tail_count = tails.len();
        let dropped = known.len().saturating_sub(reused);
        let next = StreamView::new(stream, records, tails);
        let mut signals: SignalMap = (*view.signals).clone();
        for r in fresh.iter().filter(|r| next.segments.records.contains(r)) {
            signals = absorb_record(&self.shared, &next, &signals, r)?;
        }
        for t in &fresh_tails {
            signals = absorb_tail(&signals, t);
        }
        // Compaction's output took its signals over above; only retention can
        // leave a signal pointing at a segment that has gone.
        let signals = if signals.all_live(&next) {
            Arc::new(signals)
        } else {
            rebuild(&self.shared, &next)?
        };
        let next = next.with_signals(Arc::clone(&signals));
        let segments = Arc::clone(&next.segments);
        self.install(stream, next)?;
        let persisted = persist(&self.shared, stream, &segments.records, &signals);
        let installed = self.view(stream);
        let report = StreamReport {
            stream,
            summary: SummaryState::Loaded,
            reused,
            read: unknown.len(),
            dropped,
            tails: tail_count,
            orphans_removed: remove_orphans(&self.shared, stream, &installed),
            persisted: persisted.is_ok(),
        };
        persisted.map(|()| report)
    }

    fn finish(&self, stream: StreamId, view: StreamView) -> Result<()> {
        let (segments, signals) = (Arc::clone(&view.segments), Arc::clone(&view.signals));
        self.install(stream, view)?;
        let installed = self.view(stream);
        remove_orphans(&self.shared, stream, &installed);
        persist(&self.shared, stream, &segments.records, &signals)
    }
}
