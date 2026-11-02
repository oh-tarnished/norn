//! Starting a stream's threads, at its first batch.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use norn_core::StreamId;
use norn_storage::stream_dir;

use super::buffer::Buffers;
use super::commit::Committer;
use super::dedup::Dedup;
use super::journal::{Journal, journal_dir};
use super::shared::{Shared, Work};
use super::stats::StreamStats;
use super::store;
use super::{Ingest, QUEUE_BATCHES};

/// Generations waiting behind the one being stored: one, so the commit thread,
/// then the queue, then admission feel a slow disk.
const QUEUE_GENERATIONS: usize = 1;

#[derive(Debug)]
pub(super) struct Gate {
    pub(super) dedup: Dedup,
    pub(super) tx: Option<SyncSender<Work>>,
}

/// One stream's admission state.
#[derive(Debug)]
pub(super) struct Handle {
    pub(super) shared: Arc<Shared>,
    gate: Mutex<Gate>,
}

impl Handle {
    pub(super) fn lock(&self) -> MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Ingest {
    pub(super) fn handle(&self, stream: StreamId) -> Option<Arc<Handle>> {
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(h) = streams.get(&stream) {
            return Some(Arc::clone(h));
        }
        let taken = self
            .recovered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&stream);
        let (dedup, stats) =
            taken.unwrap_or_else(|| (Dedup::new(&self.ctx.budget), StreamStats::default()));
        let buffers = Buffers::new(&self.ctx.opts);
        let shared = Arc::new(Shared::new(stream, stats, buffers));
        let tx = self.start(&shared)?;
        let gate = Mutex::new(Gate {
            dedup,
            tx: Some(tx),
        });
        let handle = Arc::new(Handle { shared, gate });
        streams.insert(stream, Arc::clone(&handle));
        Some(handle)
    }

    /// Spawns the stream's two threads. They open their writers themselves, so
    /// the first batch's `admit` never waits on a disk.
    fn start(&self, shared: &Arc<Shared>) -> Option<SyncSender<Work>> {
        let (tx, rx) = sync_channel::<Work>(QUEUE_BATCHES);
        let (store_tx, store_rx) = sync_channel(QUEUE_GENERATIONS);
        let dir = stream_dir(&self.ctx.root, shared.stream);
        let journal = journal_dir(&dir);
        let opts = self.ctx.opts;
        let (s, j, c) = (Arc::clone(shared), journal.clone(), Arc::clone(&self.ctx));
        let store = std::thread::Builder::new()
            .name("norn-store".to_owned())
            .spawn(move || store::run(&c, &s, &dir, &j, store_rx))
            .ok()?;
        let (ctx, s) = (Arc::clone(&self.ctx), Arc::clone(shared));
        let commit = std::thread::Builder::new()
            .name("norn-commit".to_owned())
            .spawn(move || match Journal::open(&journal, s.stream, &opts) {
                Ok(j) => Committer::new(ctx, s, j, store_tx).run(&rx),
                Err(e) => {
                    tracing::error!(stream = s.stream.get(), error = %e, "cannot open journal");
                    s.failed.store(true, Ordering::Release);
                }
            })
            .ok()?;
        let mut threads = self.threads.lock().unwrap_or_else(PoisonError::into_inner);
        threads.extend([store, commit]);
        Some(tx)
    }
}
