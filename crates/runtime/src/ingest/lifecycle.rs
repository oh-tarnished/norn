//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Ending the write path: refusing new batches, then closing it.

use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use super::Ingest;

impl Ingest {
    /// Refuses every new batch with `SHUTTING_DOWN`. Admitted ones finish.
    pub fn begin_drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.drained.cancel();
    }

    /// Cancelled once [`begin_drain`](Self::begin_drain) has been called: what a
    /// call waiting on its client for the next batch selects on, so an idle
    /// call ends at shutdown instead of holding its connection, and the
    /// server's graceful drain, open until the drain bound cuts it.
    #[must_use]
    pub fn draining(&self) -> tokio_util::sync::CancellationToken {
        self.drained.clone()
    }

    /// Refuses new batches, commits everything admitted, seals, joins every thread,
    /// and writes clean-shutdown markers. Blocks on the disk; idempotent; safe after
    /// a failure. Stop maintenance first, or the next start reads more than it must.
    pub fn close(&self) {
        self.begin_drain();
        let streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        for handle in streams.values() {
            handle.lock().tx = None;
        }
        drop(streams);
        let threads =
            std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        for thread in threads {
            if thread.join().is_err() {
                tracing::error!("a write-path thread panicked");
            }
        }
        self.write_clean_markers();
    }
}
