//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the write path keeps for maintenance: known damage, and totals that
//! compaction and retention change underneath it.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use norn_core::{StreamId, Timestamp};
use norn_index::IndexError;
use norn_storage::{
    Action, CleanMarker, CompactionOutcome, Corruption, RecoveryEvent, RecoveryReport, SegmentName,
    SegmentReader, StorageOptions, stream_dir,
};

use super::Ingest;
use super::stats::StreamStats;

/// Chunks of one stream known damaged: `(segment, offset)` to the samples each
/// claims. One entry per damaged chunk, so bounded by what is damaged.
type Damage = HashMap<(SegmentName, u64), u64>;

/// Damage known so far, by stream.
///
/// Concurrency: one short mutex; never held across I/O.
#[derive(Debug, Default)]
pub(crate) struct Upkeep {
    damage: Mutex<HashMap<StreamId, Damage>>,
}

impl Ingest {
    /// Applies `change` to `stream`'s totals wherever they are held: its threads'
    /// copy, and the copies waiting for them to start.
    fn change_stats(&self, stream: StreamId, change: impl Fn(&mut StreamStats)) {
        if let Some(shared) = self.running(stream) {
            change(&mut shared.stats.lock().unwrap_or_else(PoisonError::into_inner));
        }
        let mut held = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stats) = held.get_mut(&stream) {
            change(stats);
        }
        drop(held);
        let mut held = self
            .recovered
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, stats)) = held.get_mut(&stream) {
            change(stats);
        }
    }

    /// Counts the chunk at `offset` of `segment`, claiming `samples`, as damaged,
    /// once. It leaves `samples` and joins `corrupt_samples` at once, and the
    /// stream's clean marker goes, so the next start reads all of it and reports
    /// this again. Touches the disk only the first time. Idempotent.
    pub fn note_damage(&self, stream: StreamId, segment: SegmentName, offset: u64, samples: u64) {
        let mut damage = self
            .upkeep
            .damage
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let known = damage.entry(stream).or_default();
        if known.insert((segment, offset), samples).is_some() {
            return;
        }
        drop(damage);
        self.change_stats(stream, |stats| {
            stats.samples = stats.samples.saturating_sub(samples);
            stats.corrupt_samples = stats.corrupt_samples.saturating_add(samples);
        });
        tracing::warn!(stream = stream.get(), segment = %segment.file_name(), offset, samples, "damaged chunk");
        if let Err(e) = CleanMarker::remove(&stream_dir(&self.ctx.root, stream)) {
            tracing::warn!(error = %e, "cannot remove a clean marker");
        }
    }

    /// As [`note_damage`](Self::note_damage), for a region a storage call reported.
    /// The segment's directory says how many samples the chunk there claims;
    /// a region that is no chunk claims none.
    pub fn note_corruption(&self, stream: StreamId, corruption: &Corruption) {
        let Some(name) = SegmentName::parse(&corruption.path) else {
            return;
        };
        let samples = claimed(corruption, &self.ctx.opts);
        self.note_damage(stream, name, corruption.offset, samples);
    }

    /// Whether `segment` holds a chunk known damaged.
    #[must_use]
    pub fn is_damaged(&self, stream: StreamId, segment: SegmentName) -> bool {
        let damage = self
            .upkeep
            .damage
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        damage
            .get(&stream)
            .is_some_and(|d| d.keys().any(|(name, _)| *name == segment))
    }

    /// Remembers what startup verification found, and returns the samples
    /// claimed by chunks left in place damaged.
    pub(super) fn seed_damage(&self, stream: StreamId, report: &RecoveryReport) -> u64 {
        let mut damage = self
            .upkeep
            .damage
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let known = damage.entry(stream).or_default();
        let mut total = 0;
        for event in &report.events {
            let RecoveryEvent::Corruption { corruption, action } = event else {
                continue;
            };
            let Some(name) = SegmentName::parse(&corruption.path) else {
                continue;
            };
            if *action == Action::LeftInPlace {
                let samples = claimed(corruption, &self.ctx.opts);
                known.insert((name, corruption.offset), samples);
                total += samples;
            }
        }
        total
    }

    /// Tells the index and the totals of a finished compaction. Blocks on the disk.
    ///
    /// # Errors
    /// As [`compaction_applied`](Self::compaction_applied), after the totals are
    /// right: a caller refreshes the index and carries on.
    pub(crate) fn compacted(
        &self,
        stream: StreamId,
        outcome: &CompactionOutcome,
    ) -> Result<(), IndexError> {
        let (before, after) = (outcome.bytes_before, outcome.output.bytes);
        self.change_stats(stream, |stats| {
            stats.stored_bytes = stats.stored_bytes.saturating_sub(before) + after;
        });
        self.compaction_applied(stream, outcome)
    }

    /// Takes `segments`, which retention removed, out of the totals. `oldest` is
    /// the earliest sample left, if any.
    pub(crate) fn retained(
        &self,
        stream: StreamId,
        segments: &[(SegmentName, u64, u64)],
        oldest: Option<Timestamp>,
    ) {
        let mut damage = self
            .upkeep
            .damage
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let known = damage.entry(stream).or_default();
        let (mut corrupt, mut samples, mut bytes) = (0, 0, 0);
        for (name, held, size) in segments {
            let mut lost = 0;
            known.retain(|(segment, _), n| {
                let same = segment == name;
                lost += if same { *n } else { 0 };
                !same
            });
            corrupt += lost;
            samples += held.saturating_sub(lost);
            bytes += size;
        }
        drop(damage);
        self.change_stats(stream, |stats| {
            stats.samples = stats.samples.saturating_sub(samples);
            stats.corrupt_samples = stats.corrupt_samples.saturating_sub(corrupt);
            stats.stored_bytes = stats.stored_bytes.saturating_sub(bytes);
            stats.time_range = stats
                .time_range
                .map(|(from, to)| (oldest.unwrap_or(from).max(from), to));
        });
    }
}

/// The samples the directory says the chunk at `corruption` holds.
fn claimed(corruption: &Corruption, opts: &StorageOptions) -> u64 {
    let Ok(reader) = SegmentReader::open(&corruption.path, *opts) else {
        return 0;
    };
    let found = reader
        .chunks()
        .iter()
        .find(|c| c.offset == corruption.offset);
    found.map_or(0, |c| u64::from(c.count))
}
