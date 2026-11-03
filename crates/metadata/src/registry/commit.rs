//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Group commit: many mutations, one fsync, and rewriting a mostly dead log.
//!
//! One committer at a time is the *leader*: it marks a sync in progress, notes
//! how far the log is written, and fsyncs with no lock held. Everyone else waits
//! on a condition variable. When the fsync returns, the leader publishes the new
//! durable position and wakes every waiter at once; each checks its own position
//! and returns if covered, and of those still uncovered one leads the next fsync.
//! A waiter never queues behind an fsync its record does not need. With a mutex
//! held across the fsync, as before, waiters were released one at a time, and a
//! covered one could wait out the next leader's fsync too, so throughput stopped
//! rising past 16 concurrent creates (docs/performance.md, "Provisioning").

use std::fs::File;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard, PoisonError};

use super::Registry;
use crate::error::MetadataError;
use crate::op::Op;

/// The handle fsyncs go through, and whether one is under way.
///
/// Concurrency: guarded by the registry's `syncer` mutex; `file` is an `Arc` so
/// a leader can fsync it after releasing the mutex.
#[derive(Debug)]
pub(crate) struct Syncer {
    file: Arc<File>,
    syncing: bool,
}

impl Syncer {
    pub(crate) fn new(file: File) -> Self {
        Self {
            file: Arc::new(file),
            syncing: false,
        }
    }
}

impl Registry {
    /// Makes every mutation so far durable. Costs one atomic load when they
    /// already are; the write path calls it ahead of each fsync, so a durable
    /// sample never names a signal that a power loss could forget.
    ///
    /// # Errors
    /// [`MetadataError::Poisoned`]; [`MetadataError::Io`], which poisons.
    pub fn commit(&self) -> Result<(), MetadataError> {
        self.commit_to(self.written.load(Ordering::Acquire))
    }

    /// Appends `ops` to the log in one write, as one all-or-nothing batch, for a
    /// caller holding the write lock on `state` (lock order: state, then log).
    /// Returns the position of the last. Not yet durable.
    pub(crate) fn log_ops(&self, ops: &[Op]) -> Result<u64, MetadataError> {
        self.refuse_if_poisoned()?;
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(e) = log.append_all(ops) {
            self.poisoned.store(true, Ordering::Release);
            return Err(e);
        }
        let n = ops.len() as u64;
        Ok(self.written.fetch_add(n, Ordering::AcqRel) + n)
    }

    /// As [`Registry::log_ops`], for one record.
    pub(crate) fn log_op(&self, op: &Op) -> Result<u64, MetadataError> {
        self.log_ops(std::slice::from_ref(op))
    }

    pub(crate) fn refuse_if_poisoned(&self) -> Result<(), MetadataError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(MetadataError::Poisoned);
        }
        Ok(())
    }

    fn lock_syncer(&self) -> MutexGuard<'_, Syncer> {
        self.syncer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits until position `lsn` is durable, leading an fsync if none is under
    /// way. Only a leader then checks whether the log needs rewriting.
    pub(crate) fn commit_to(&self, lsn: u64) -> Result<(), MetadataError> {
        if self.synced.load(Ordering::Acquire) >= lsn {
            return self.refuse_if_poisoned();
        }
        let mut g = self.lock_syncer();
        loop {
            self.refuse_if_poisoned()?;
            if self.synced.load(Ordering::Acquire) >= lsn {
                return Ok(());
            }
            if !g.syncing {
                break;
            }
            g = self.durable.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
        g.syncing = true;
        let file = Arc::clone(&g.file);
        // Read under the mutex, after our own append: covers `lsn`.
        let target = self.written.load(Ordering::Acquire);
        drop(g);
        let flushed = file.sync_data();
        let mut g = self.lock_syncer();
        g.syncing = false;
        match &flushed {
            Ok(()) => {
                self.synced.fetch_max(target, Ordering::AcqRel);
            }
            Err(_) => self.poisoned.store(true, Ordering::Release),
        }
        drop(g);
        self.durable.notify_all();
        if let Err(source) = flushed {
            let path = self.log.lock().unwrap_or_else(PoisonError::into_inner);
            let path = path.path.clone();
            return Err(MetadataError::Io {
                op: "fsync",
                path,
                source,
            });
        }
        self.maybe_rewrite()
    }

    /// Rewrites the log once it holds more than twice the live records plus
    /// 1,024, so it stays proportional to what exists. Takes state, then log,
    /// then syncer, and waits out an fsync under way: appends and commits wait
    /// for it, and everything is durable when it finishes.
    fn maybe_rewrite(&self) -> Result<(), MetadataError> {
        let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        if log.records <= state.live().saturating_mul(2).saturating_add(1024) {
            return Ok(());
        }
        let mut g = self.lock_syncer();
        while g.syncing {
            g = self.durable.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
        let written = self.written.load(Ordering::Acquire);
        let rewritten = log.rewrite(&state.snapshot()).and_then(|()| {
            g.file = Arc::new(log.sync_handle()?);
            Ok(())
        });
        match &rewritten {
            Ok(()) => {
                self.synced.fetch_max(written, Ordering::AcqRel);
            }
            Err(_) => self.poisoned.store(true, Ordering::Release),
        }
        drop(g);
        self.durable.notify_all();
        rewritten
    }
}
