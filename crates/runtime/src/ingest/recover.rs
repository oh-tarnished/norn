//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Startup recovery of the write path: docs/reference/storage.md, "Recovery at startup".

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use norn_core::{Sample, StreamId};
use norn_index::TemporalIndex;
use norn_storage::{
    CleanMarker, RecoveryMode, SegmentReader, StorageError, StorageOptions, StreamWriter,
    live_segments, recover_stream, recover_stream_vouched, stream_dir, sync_dir,
};

use super::buffer::Buffers;
use super::dedup::Dedup;
use super::indexing::{Publisher, RecoverError};
use super::journal::{journal_dir, journal_options};
use super::report::{IndexSummary, IngestRecovery};
use super::scan::{key, reconcile};
use super::shared::Shared;
use super::stats::StreamStats;
use super::store::store;
use super::{Ingest, record};

impl Ingest {
    /// Recovers every stream the registry knows, before any batch is admitted.
    /// Blocks on the disk. A stream whose clean marker says nothing has changed
    /// costs one small file and a chunk per signal; any other reads and checks
    /// every chunk its marker does not vouch for, and every directory.
    ///
    /// Idempotent over a crash: a crash here repeats it, and it finds what the
    /// last attempt wrote. Each stream's locks are taken and released inside.
    /// Opens the temporal index last, over the recovered segments.
    ///
    /// # Errors
    /// [`RecoverError::Storage`] for what stops a stream being assessed, such as
    /// a segment from a newer Norn; [`RecoverError::Index`] when the index cannot
    /// open, as when it needs more than `index_bytes`. The daemon must not serve
    /// then.
    pub fn recover(&self) -> Result<IngestRecovery, RecoverError> {
        let mut total = IngestRecovery::default();
        let ids = self.ctx.registry.view().stream_ids();
        let mut clean = Vec::new();
        for id in ids {
            total.streams += 1;
            let dir = stream_dir(&self.ctx.root, id);
            if !dir.exists() {
                continue;
            }
            if self.clean_start(id, &dir, &mut total, &mut clean)? {
                continue;
            }
            let mut dedup = Dedup::new(&self.ctx.budget);
            let stats = self.recover_one(id, &dir, &mut dedup, &mut total)?;
            let mut map = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
            map.insert(id, stats);
            drop(map);
            let mut held = self
                .recovered
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            held.insert(id, (dedup, stats));
        }
        let (index, report) = TemporalIndex::open(&self.ctx.root, self.ctx.index_options)?;
        total.index = IndexSummary::from(&report);
        let _ = self.ctx.index.set(Arc::new(index));
        self.seed_clean(clean);
        Ok(total)
    }

    fn recover_one(
        &self,
        id: StreamId,
        dir: &Path,
        dedup: &mut Dedup,
        total: &mut IngestRecovery,
    ) -> Result<StreamStats, StorageError> {
        let opts = self.ctx.opts;
        let marker = CleanMarker::load(dir, id);
        let vouched = |name, len| marker.as_ref().is_some_and(|m| m.vouches(name, len));
        let repair = RecoveryMode::Repair;
        let segments = recover_stream_vouched(dir, &opts, repair, &vouched)?;
        total.absorb(&segments);
        total.verified_segments += segments.verified_segments;
        total.vouched_segments += segments.vouched_segments;
        let journal = journal_dir(dir);
        let mut samples = Vec::new();
        let mut consumed = Vec::new();
        if journal.exists() {
            let report = recover_stream(&journal, &journal_options(&opts), RecoveryMode::Repair)?;
            total.absorb(&report);
            for segment in live_segments(&journal)? {
                let whole = read_journal(&segment.path, &opts, &mut samples, total)?;
                if whole {
                    consumed.push(segment.path);
                } else {
                    set_aside(&segment.path, dir)?;
                }
            }
        }
        let mut fresh: HashSet<_> = samples.iter().map(key).collect();
        let found = fresh.len();
        reconcile(dir, &opts, &mut fresh, &self.ctx.registry, dedup)?;
        total.duplicate_samples += (found - fresh.len()) as u64;
        let mut buffers = Buffers::new(&opts);
        let mut stats = StreamStats::default();
        for s in &segments.segments {
            stats.add(s.samples, s.bytes, s.time_range);
        }
        let damaged = self.seed_damage(id, &segments);
        stats.samples = stats.samples.saturating_sub(damaged);
        stats.corrupt_samples = damaged;
        for s in &samples {
            dedup.admit(s);
            self.ctx.registry.mark_stored(s.signal);
            if fresh.contains(&key(s)) {
                buffers.push(s)?;
                total.replayed_samples += 1;
                stats.buffer(1, Some((s.timestamp, s.timestamp)));
            }
        }
        let shared = Shared::new(id, stats, Buffers::new(&opts));
        let mut writer = StreamWriter::open(dir, id, opts)?;
        let mut none = Publisher::new(None, &shared);
        let generation = buffers.seal(consumed);
        store(&mut writer, generation, &opts, &journal, &shared, &mut none)?;
        writer.close()?;
        let stats = *shared.stats.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(stats)
    }
}

/// Reads every batch of a journal segment into `samples`. Returns false if any
/// could not be read: recovery reported it, and the segment must then be kept.
fn read_journal(
    path: &Path,
    opts: &StorageOptions,
    samples: &mut Vec<Sample>,
    total: &mut IngestRecovery,
) -> Result<bool, StorageError> {
    let mut reader = SegmentReader::open(path, journal_options(opts))?;
    let mut whole = true;
    for meta in reader.chunks().to_vec() {
        let batch = match reader.read_chunk(&meta) {
            Ok(chunk) => chunk.sample(0).and_then(|s| match s.value {
                norn_core::Value::Bytes(bytes) => record::decode(&bytes),
                _ => None,
            }),
            Err(e) => {
                total.note(&e);
                None
            }
        };
        match batch {
            Some(batch) => {
                total.replayed_batches += 1;
                samples.extend(batch);
            }
            None => whole = false,
        }
    }
    Ok(whole)
}

/// Moves an unreadable journal segment to quarantine, whole: it is never deleted.
fn set_aside(path: &Path, dir: &Path) -> Result<(), StorageError> {
    let quarantine: PathBuf = dir.join(norn_storage::QUARANTINE_DIR);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let to = quarantine.join(format!("journal-{}", name.unwrap_or_default()));
    let io = |op, source| StorageError::Io {
        op,
        path: to.clone(),
        source,
    };
    std::fs::create_dir_all(&quarantine).map_err(|e| io("create directory", e))?;
    std::fs::rename(path, &to).map_err(|e| io("rename", e))?;
    sync_dir(&quarantine)
}
