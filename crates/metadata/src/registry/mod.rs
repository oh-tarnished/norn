//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry: the records in memory, the log on disk, and group commit.

mod batch;
mod batch_signals;
mod commit;
mod entity;
#[cfg(test)]
mod lock_tests;
mod signal;
mod signal_update;
mod spec;
mod state;
mod stream;
mod view;

pub use batch::MAX_BATCH_CREATE;
pub use spec::{EntityPatch, EntitySpec, SignalPatch, SignalSpec, StreamPatch, StreamSpec};
pub use view::{RegistryView, SignalRoute};

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Condvar, Mutex, PoisonError, RwLock};

use norn_core::{Limits, MetadataLimits};

use self::commit::Syncer;
use self::state::State;
use crate::error::MetadataError;
use crate::log::{Log, LogRecovery};

/// Whether a mutation takes effect (AIP `validate_only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Write {
    /// Apply and persist it.
    #[default]
    Commit,
    /// Check everything and return what it would produce; change nothing.
    ValidateOnly,
}

/// The durable catalogue of entities, signals and streams.
///
/// Lifecycle: [`Registry::open`] takes the directory's lock, replays the log and
/// spawns nothing; dropping releases the lock. There is no `close`: every
/// acknowledged mutation is already on disk.
///
/// Durability: a mutation returns only after its record is fsynced. Others that
/// run meanwhile share that fsync and all wake when it returns, so concurrent
/// creates cost one disk flush between them, and throughput rises with
/// concurrency. A batch ([`Registry::create_signals`]) is one write and one
/// fsync, and survives a crash whole or not at all. A change is visible to
/// readers just before it is durable, so whoever makes data depend on a record
/// calls [`Registry::commit`] first.
///
/// Memory: every record, bounded by the limits it was opened with. Concurrency:
/// `Send + Sync`. Reads never touch the disk; mutations block on it, and must
/// not be called on an async executor's thread.
#[derive(Debug)]
pub struct Registry {
    pub(crate) state: RwLock<State>,
    pub(crate) log: Mutex<Log>,
    pub(crate) syncer: Mutex<Syncer>,
    /// Signalled whenever an fsync or a rewrite finishes.
    pub(crate) durable: Condvar,
    pub(crate) written: AtomicU64,
    pub(crate) synced: AtomicU64,
    pub(crate) poisoned: AtomicBool,
    pub(crate) limits: Limits,
    metadata: MetadataLimits,
    _lock: File,
}

impl Drop for Registry {
    fn drop(&mut self) {
        // Closing our descriptor alone does not release a Unix flock if a
        // concurrent process spawn temporarily inherited the open description.
        // File drop still closes the handle if the explicit unlock fails.
        let _ = self._lock.unlock();
    }
}

impl Registry {
    /// Opens the registry in directory `dir`, creating it if missing.
    ///
    /// # Errors
    /// [`MetadataError::Locked`] if another registry holds it;
    /// [`MetadataError::Corrupt`] or [`MetadataError::UnsupportedVersion`] for a
    /// log it must not guess about; [`MetadataError::Io`].
    pub fn open(
        dir: &Path,
        limits: Limits,
        metadata: MetadataLimits,
    ) -> Result<(Self, LogRecovery), MetadataError> {
        let err = |op, source| MetadataError::Io {
            op,
            path: dir.to_path_buf(),
            source,
        };
        fs::create_dir_all(dir).map_err(|e| err("create directory", e))?;
        let lock_path = dir.join("LOCK");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| err("open lock", e))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(MetadataError::Locked { path: lock_path });
            }
            Err(TryLockError::Error(e)) => return Err(err("lock", e)),
        }
        let mut state = State::default();
        let (log, report) = Log::open(&dir.join("metadata.log"), |op| state.apply(op))?;
        let syncer = log.sync_handle()?;
        let registry = Self {
            state: RwLock::new(state),
            log: Mutex::new(log),
            syncer: Mutex::new(Syncer::new(syncer)),
            durable: Condvar::new(),
            written: AtomicU64::new(0),
            synced: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            limits,
            metadata,
            _lock: lock,
        };
        Ok((registry, report))
    }

    /// The bounds new annotations must respect; build maps with these.
    #[must_use]
    pub fn metadata_limits(&self) -> MetadataLimits {
        self.metadata
    }

    /// The resource ID of the stream with compact ID `id`, if it exists.
    #[must_use]
    pub fn stream_name(&self, id: norn_core::StreamId) -> Option<String> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        st.stream_ids.get(&id).cloned()
    }

    /// Takes the shared lock, for resolving a batch and admitting it as one step
    /// (see [`RegistryView`]). Never touches the disk.
    pub fn view(&self) -> RegistryView<'_> {
        RegistryView {
            state: self.state.read().unwrap_or_else(PoisonError::into_inner),
        }
    }

    /// Records that `signal` holds stored samples, which refuses its deletion.
    /// For startup, from what recovery found. Unknown signals are ignored.
    pub fn mark_stored(&self, signal: norn_core::SignalId) {
        self.view().mark_stored(signal);
    }
}
