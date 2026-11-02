//! A read lock on the registry, held while a batch is resolved and admitted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLockReadGuard};

use norn_core::{SignalId, StreamId, ValueKind};

use super::state::State;
use crate::record::StreamRecord;

/// What the write path needs per sample, found in O(1) by resource name.
#[derive(Debug, Clone)]
pub struct SignalRoute {
    /// The compact ID samples are stored under.
    pub id: SignalId,
    /// The value kind every sample must carry.
    pub kind: ValueKind,
    /// The stream it records into.
    pub stream: StreamId,
    pub(super) has_data: Arc<AtomicBool>,
}

impl SignalRoute {
    /// Records that this signal holds data, refusing its deletion. Call it
    /// while holding the [`RegistryView`] it came from, before releasing it: a
    /// delete needs the write lock, so it either ran before this batch resolved
    /// the signal, or it sees this mark.
    pub fn mark_data(&self) {
        if !self.has_data.load(Ordering::Relaxed) {
            self.has_data.store(true, Ordering::Release);
        }
    }

    pub(super) fn holds_data(&self) -> bool {
        self.has_data.load(Ordering::Acquire)
    }
}

/// A snapshot-consistent read of the registry.
///
/// Holding one blocks mutations and no reads; hold it for a synchronous step and
/// never across an `.await` (the guard is deliberately not `Send`).
#[derive(Debug)]
pub struct RegistryView<'a> {
    pub(super) state: RwLockReadGuard<'a, State>,
}

impl RegistryView<'_> {
    /// The route of `entities/{entity}/signals/{signal}`, by full name.
    #[must_use]
    pub fn route(&self, signal_name: &str) -> Option<&SignalRoute> {
        self.state.routes.get(signal_name)
    }

    /// The stream with resource ID `id`.
    #[must_use]
    pub fn stream(&self, id: &str) -> Option<&StreamRecord> {
        self.state.streams.get(id)
    }

    /// The compact ID of every stream, for startup recovery.
    #[must_use]
    pub fn stream_ids(&self) -> Vec<StreamId> {
        self.state.stream_ids.keys().copied().collect()
    }

    /// See [`crate::Registry::mark_stored`].
    pub(super) fn mark_stored(&self, signal: SignalId) {
        let name = self
            .state
            .signal_ids
            .get(&signal)
            .map(|(e, s)| super::state::signal_name(e, s));
        if let Some(route) = name.and_then(|n| self.state.routes.get(&n)) {
            route.mark_data();
        }
    }
}
