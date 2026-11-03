//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The handle: opening, taking snapshots, installing updates.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use norn_core::StreamId;
use norn_storage::stream_dir;

use crate::cache::PageCache;
use crate::error::{IndexError, Result};
use crate::options::IndexOptions;
use crate::reconcile::{open_stream, persist};
use crate::report::IndexReport;
use crate::shared::Shared;
use crate::snapshot::Snapshot;
use crate::stats::{Counters, IndexStats};
use crate::view::StreamView;

/// The temporal index of one database (SPEC §21).
///
/// **Lifecycle.** [`TemporalIndex::open`] after `norn_storage::recover_stream`
/// has run on every stream; it starts no task that outlives it. Dropping it is
/// the whole of closing: nothing here needs flushing, since the files are
/// derived and written as updates happen.
///
/// **Concurrency.** `Send + Sync`; share it in an `Arc`. [`snapshot`](Self::snapshot)
/// holds a read lock for one `Arc` clone, and the write lock is held for one
/// pointer store, so neither a reader nor ingest waits on the other's I/O. Updates
/// are serialised by a separate mutex, and do their I/O under that one only.
///
/// **What a reader sees during compaction.** A snapshot holds the inputs or the
/// output, never both and never neither, and the switch is one pointer store.
/// `compact` deletes its inputs before the index hears of them, so an old snapshot
/// can name files that have gone: `ChunkFile::open` then fails with `NotFound`,
/// or a plan yields [`IndexError::SegmentGone`]. Either way call
/// [`refresh`](Self::refresh), which is idempotent, then plan again from the last
/// timestamp delivered per signal, dropping anything already delivered: chunk
/// boundaries have moved, so an old locator means nothing in the new plan.
///
/// **Failure.** See the crate docs. A bad index file is a rebuild, not an error.
///
/// **Memory.** Segment lists and tails are charged first, and the page cache gets
/// the remainder of [`IndexOptions::memory_budget_bytes`].
#[derive(Debug)]
pub struct TemporalIndex {
    pub(crate) shared: Arc<Shared>,
    current: RwLock<Arc<Snapshot>>,
    pub(crate) updates: Mutex<()>,
}

impl TemporalIndex {
    /// Opens the index of the database at `root`, reconciling every stream
    /// directory with its `summary.nidx`.
    ///
    /// Cost: one `read_dir` and one `stat` per segment plus each summary when
    /// current; every segment directory when not, on up to `rebuild_threads`
    /// scoped threads, all joined before this returns.
    ///
    /// # Errors
    /// [`IndexError::Storage`] when a segment does not verify (run recovery
    /// first); [`IndexError::Io`]; [`IndexError::OverBudget`];
    /// [`IndexError::WrongStream`]. A damaged index file is none of these.
    pub fn open(root: &Path, options: IndexOptions) -> Result<(Self, IndexReport)> {
        let shared = Arc::new(Shared {
            root: root.to_path_buf(),
            options,
            cache: PageCache::default(),
            counters: Counters::default(),
        });
        let mut streams = BTreeMap::new();
        let mut report = IndexReport::default();
        for stream in list_streams(root)? {
            let dir = stream_dir(root, stream);
            let (view, mut found) = open_stream(&shared, stream, &dir)?;
            if !found.clean() {
                let records = &view.segments.records;
                found.persisted = persist(&shared, stream, records, &view.signals).is_ok();
            }
            streams.insert(stream, Arc::new(view));
            report.streams.push(found);
        }
        let needed: u64 = streams.values().map(|v| v.memory_bytes()).sum();
        let budget = options.memory_budget_bytes;
        if needed > budget {
            return Err(IndexError::OverBudget { needed, budget });
        }
        shared.cache.set_capacity(budget - needed);
        let snapshot = Snapshot {
            epoch: 0,
            streams,
            shared: Arc::clone(&shared),
        };
        let index = Self {
            shared,
            current: RwLock::new(Arc::new(snapshot)),
            updates: Mutex::new(()),
        };
        Ok((index, report))
    }

    /// The current state. Cost: one `Arc` clone. Never blocks on I/O.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        let current = self.current.read().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(&current)
    }

    /// What the index has done since it opened.
    #[must_use]
    pub fn stats(&self) -> IndexStats {
        self.shared.counters.snapshot()
    }

    /// Empties the page cache. For benchmarks and tests of cold behaviour; the
    /// cache needs no help in normal running.
    pub fn clear_cache(&self) {
        self.shared.cache.clear();
    }

    /// Bytes charged to the budget now: lists, tails and the page cache.
    #[must_use]
    pub fn memory_bytes(&self) -> u64 {
        let snapshot = self.snapshot();
        let views = snapshot.streams.values();
        views.map(|v| v.memory_bytes()).sum::<u64>() + self.shared.cache.used()
    }

    /// Installs `view` as `stream`'s state, or refuses it unchanged.
    ///
    /// # Errors
    /// [`IndexError::OverBudget`].
    pub(crate) fn install(&self, stream: StreamId, view: StreamView) -> Result<()> {
        let current = self.snapshot();
        let mut streams = current.streams.clone();
        streams.insert(stream, Arc::new(view));
        let needed: u64 = streams.values().map(|v| v.memory_bytes()).sum();
        let budget = self.shared.options.memory_budget_bytes;
        if needed > budget {
            return Err(IndexError::OverBudget { needed, budget });
        }
        self.shared.cache.set_capacity(budget - needed);
        let next = Arc::new(Snapshot {
            epoch: current.epoch + 1,
            streams,
            shared: Arc::clone(&self.shared),
        });
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = next;
        Ok(())
    }
}

/// The streams with a directory under `root`, in id order. A foreign name is
/// ignored.
fn list_streams(root: &Path) -> Result<Vec<StreamId>> {
    let dir: PathBuf = root.join("streams");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            let (op, path) = ("list", dir);
            return Err(IndexError::Io { op, path, source });
        }
    };
    let mut out: Vec<StreamId> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            let raw = (name.len() == 16).then(|| u64::from_str_radix(name, 16).ok())??;
            Some(StreamId::new(raw))
        })
        .collect();
    out.sort_unstable();
    Ok(out)
}
