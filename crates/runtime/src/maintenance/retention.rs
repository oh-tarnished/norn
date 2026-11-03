//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Applying a stream's retention policy: explicit, logged, whole segments.

use std::sync::{Arc, PoisonError};

use norn_core::{StreamId, Timestamp};
use norn_metadata::{Registry, Retention};
use norn_storage::{SegmentName, remove_segments, stream_dir};

use super::Maintenance;
use super::ops::Failure;
use super::plan::retention;
use super::report::Removal;

/// The most segments a report lists; the counts cover all.
const MAX_LISTED: usize = 1000;

/// One whole segment removed, or that would be.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RemovedSegment {
    /// The segment.
    pub name: SegmentName,
    /// Its earliest sample.
    pub oldest: Timestamp,
    /// Its latest sample.
    pub newest: Timestamp,
    /// Samples it held.
    pub samples: u64,
    /// Bytes it took.
    pub bytes: u64,
}

/// What a retention pass removed, or under `dry_run` would have.
///
/// Concurrency: owned plain data. Memory: at most 1,000 segments listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetentionReport {
    /// Whether anything was removed.
    pub applied: bool,
    /// Samples older than this were eligible; `None` for a size limit or no policy.
    pub cutoff: Option<Timestamp>,
    /// Segments removed.
    pub segments: u64,
    /// Samples they held.
    pub samples: u64,
    /// Bytes they took.
    pub bytes: u64,
    /// Bytes of sealed segments left.
    pub retained_bytes: u64,
    /// The segments, oldest first, at most 1,000.
    pub removed: Vec<RemovedSegment>,
}

/// `stream`'s policy, from the registry at this moment.
fn policy(registry: &Registry, stream: StreamId) -> Option<Retention> {
    let name = registry.stream_name(stream)?;
    registry.stream(&name).ok().map(|r| r.retention)
}

impl Maintenance {
    /// Runs `stream`'s policy. Under `dry_run` this is pure reading.
    ///
    /// Otherwise it logs what it will remove first, at `WARN`, deletes oldest first
    /// so a crash leaves a pass unfinished rather than a hole, reconciles the index
    /// with `refresh`, and counts the removal for `GetServer`. A reader holding
    /// a removed file keeps reading it; one arriving after gets a smaller answer.
    ///
    /// # Errors
    /// [`Failure::Failed`] when a file cannot be removed; those before it are gone
    /// and accounted for at the next start.
    pub(super) fn apply_retention(
        &self,
        stream: StreamId,
        dry_run: bool,
    ) -> Result<RetentionReport, Failure> {
        let mut report = RetentionReport {
            applied: !dry_run,
            ..RetentionReport::default()
        };
        let (Some(index), Some(rule)) = (
            self.ingest.index().map(Arc::clone),
            policy(self.registry.as_ref(), stream),
        ) else {
            return Ok(report);
        };
        let snapshot = index.snapshot();
        let records = snapshot.segments(stream);
        let newest = self.ingest.stats(stream).time_range.map(|(_, to)| to);
        let (gone, cutoff) = retention(records, rule, newest);
        report.cutoff = cutoff;
        report.segments = gone.len() as u64;
        report.samples = gone.iter().map(|r| r.samples).sum();
        report.bytes = gone.iter().map(|r| r.file_len).sum();
        let total: u64 = records.iter().map(|r| r.file_len).sum();
        report.retained_bytes = total.saturating_sub(report.bytes);
        let listed = gone.iter().take(MAX_LISTED).map(|r| RemovedSegment {
            name: r.name(),
            oldest: r.min_ts,
            newest: r.max_ts,
            samples: r.samples,
            bytes: r.file_len,
        });
        report.removed = listed.collect();
        if dry_run || gone.is_empty() {
            return Ok(report);
        }
        tracing::warn!(
            stream = stream.get(),
            segments = report.segments,
            samples = report.samples,
            bytes = report.bytes,
            cutoff = ?cutoff.map(Timestamp::as_nanos),
            policy = ?rule,
            "retention removing segments"
        );
        let dir = stream_dir(self.ingest.root(), stream);
        let paths: Vec<_> = gone
            .iter()
            .map(|r| dir.join(r.name().file_name()))
            .collect();
        remove_segments(&dir, &paths).map_err(|e| {
            self.refresh(stream);
            Failure::Failed(e.to_string())
        })?;
        self.refresh(stream);
        let held: Vec<_> = gone
            .iter()
            .map(|r| (r.name(), r.samples, r.file_len))
            .collect();
        self.ingest
            .retained(stream, &held, self.ingest.oldest_stored(stream));
        let mut recent = self
            .counters
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        recent.push_front(Removal {
            stream: self.registry.stream_name(stream).unwrap_or_default(),
            at: self.clock.now(),
            segments: report.segments,
            samples: report.samples,
            bytes: report.bytes,
        });
        recent.truncate(super::report::MAX_RECENT);
        drop(recent);
        self.counters.removed(&report);
        tracing::warn!(
            stream = stream.get(),
            segments = report.segments,
            "retention removed segments"
        );
        Ok(report)
    }
}
