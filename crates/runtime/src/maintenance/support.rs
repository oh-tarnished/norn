//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What every job needs: an index that agrees with the directory, and memory.

use norn_core::StreamId;
use norn_index::{IndexError, SegmentRecord};

use super::Maintenance;
use super::ops::Failure;

impl Maintenance {
    /// Reconciles the index with the directory. After a failure past the commit
    /// the output is what is live.
    pub(super) fn refresh(&self, stream: StreamId) {
        let Some(index) = self.ingest.index() else {
            return;
        };
        match index.refresh(stream) {
            Ok(_) | Err(IndexError::PersistFailed(_)) => {}
            Err(e) => tracing::warn!(stream = stream.get(), error = %e, "index refresh failed"),
        }
    }

    /// Reserves one run's working memory: its directories, a merged chunk, an
    /// input chunk and the encoded frame, or the whole budget if that is less.
    /// Waits for no one: the worker is the budget's only user.
    pub(super) fn reserve(&self, run: &[SegmentRecord]) -> Result<crate::Reservation, Failure> {
        const DIR_ENTRY: u64 = 44;
        const DECODED_SAMPLE: u64 = 40;
        let opts = self.ingest.storage_options();
        let chunks: u64 = run.iter().map(|r| u64::from(r.chunks)).sum();
        let chunk = opts.max_chunk_samples as u64 * DECODED_SAMPLE;
        let wanted = chunks * DIR_ENTRY + chunk + 2 * u64::from(opts.max_chunk_bytes);
        let bytes = wanted.min(self.memory.capacity());
        self.memory
            .try_reserve(bytes)
            .map_err(|e| Failure::Failed(e.to_string()))
    }
}
