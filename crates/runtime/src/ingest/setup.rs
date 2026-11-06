//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What an [`Ingest`] is given before it starts: its keys and its fsync gate.

use std::sync::Arc;

use super::{Ingest, SyncGate};

impl Ingest {
    /// Encrypts and reads everything under `keys`, or nothing when `None`. Before
    /// recovery and any batch: no effect once a stream has started.
    #[must_use]
    pub fn keyring(mut self, keys: Option<&'static norn_storage::Keyring>) -> Self {
        if let Some(ctx) = Arc::get_mut(&mut self.ctx) {
            ctx.opts.keyring = keys;
            ctx.index_options.storage.keyring = keys;
        }
        self
    }

    /// Installs `gate` around every journal fsync. Before any batch only: it
    /// has no effect once a stream has started.
    #[must_use]
    pub fn sync_gate(mut self, gate: Arc<dyn SyncGate>) -> Self {
        if let Some(ctx) = Arc::get_mut(&mut self.ctx) {
            ctx.gate = Some(gate);
        }
        self
    }
}
