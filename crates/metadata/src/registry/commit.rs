//! Group commit: many mutations, one fsync, and rewriting a mostly dead log.

use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use super::Registry;
use crate::error::MetadataError;
use crate::op::Op;

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

    /// Appends `op` to the log, for a caller holding the write lock on `state`
    /// (lock order: state, then log), and returns its position. Not yet durable.
    pub(crate) fn log_op(&self, op: &Op) -> Result<u64, MetadataError> {
        self.refuse_if_poisoned()?;
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(e) = log.append(op) {
            self.poisoned.store(true, Ordering::Release);
            return Err(e);
        }
        Ok(self.written.fetch_add(1, Ordering::AcqRel) + 1)
    }

    pub(crate) fn refuse_if_poisoned(&self) -> Result<(), MetadataError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(MetadataError::Poisoned);
        }
        Ok(())
    }

    /// Waits until position `lsn` is durable, fsyncing if nobody else has.
    /// Whoever holds the lock covers everything written so far, so callers
    /// queued behind it usually find their work done (group commit). Then
    /// rewrites the log if it has become mostly dead.
    pub(crate) fn commit_to(&self, lsn: u64) -> Result<(), MetadataError> {
        if self.synced.load(Ordering::Acquire) >= lsn {
            return self.refuse_if_poisoned();
        }
        let file = self.syncer.lock().unwrap_or_else(PoisonError::into_inner);
        if self.synced.load(Ordering::Acquire) >= lsn {
            return self.refuse_if_poisoned();
        }
        let target = self.written.load(Ordering::Acquire);
        if let Err(source) = file.sync_data() {
            self.poisoned.store(true, Ordering::Release);
            let path = self
                .log
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .path
                .clone();
            return Err(MetadataError::Io {
                op: "fsync",
                path,
                source,
            });
        }
        self.synced.store(target, Ordering::Release);
        drop(file);
        self.maybe_rewrite()
    }

    /// Rewrites the log once it holds more than twice the live records plus
    /// 1,024, so it stays proportional to what exists. Takes state, then log,
    /// then syncer: appends and fsyncs wait for it, and everything is durable
    /// when it finishes.
    fn maybe_rewrite(&self) -> Result<(), MetadataError> {
        let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        if log.records <= state.live().saturating_mul(2).saturating_add(1024) {
            return Ok(());
        }
        let mut file = self.syncer.lock().unwrap_or_else(PoisonError::into_inner);
        let written = self.written.load(Ordering::Acquire);
        let rewritten = log.rewrite(&state.snapshot()).and_then(|()| {
            *file = log.sync_handle()?;
            Ok(())
        });
        if rewritten.is_err() {
            self.poisoned.store(true, Ordering::Release);
        }
        self.synced.store(written, Ordering::Release);
        rewritten
    }
}
