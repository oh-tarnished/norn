//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The records in memory, and applying a log record to them.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use norn_core::{EntityId, SignalId, StreamId};

use super::view::SignalRoute;
use crate::op::Op;
use crate::record::{EntityRecord, SignalRecord, StreamRecord};

/// Every resource, with the indexes a read or an append needs.
///
/// Memory: bounded by the registry's limits. `BTreeMap`s give the name order
/// that listing and pagination need; the `HashMap`s give compact-ID lookups and
/// the O(1) name lookup an append does per sample.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) entities: BTreeMap<String, EntityRecord>,
    pub(crate) streams: BTreeMap<String, StreamRecord>,
    /// By (entity resource ID, signal resource ID).
    pub(crate) signals: BTreeMap<(String, String), SignalRecord>,
    pub(crate) entity_ids: HashMap<EntityId, String>,
    pub(crate) stream_ids: HashMap<StreamId, String>,
    pub(crate) signal_ids: HashMap<SignalId, (String, String)>,
    /// By full resource name, `entities/{entity}/signals/{signal}`.
    pub(crate) routes: HashMap<String, SignalRoute>,
    /// Allocated next: entity, signal, stream. IDs are never reused, even after
    /// a delete; stored data outlives any confusion about what an ID meant.
    pub(crate) next: [u64; 3],
}

pub(crate) fn signal_name(entity: &str, signal: &str) -> String {
    format!("entities/{entity}/signals/{signal}")
}

impl State {
    /// How many records a fresh log would need for all of this.
    pub(crate) fn live(&self) -> u64 {
        (self.entities.len() + self.streams.len() + self.signals.len()) as u64 + 1
    }

    /// Records that reproduce this state in a fresh log.
    pub(crate) fn snapshot(&self) -> Vec<Op> {
        let entities = self.entities.values().cloned().map(Op::PutEntity);
        let streams = self.streams.values().cloned().map(Op::PutStream);
        let signals = self.signals.values().cloned().map(Op::PutSignal);
        std::iter::once(Op::NextIds(self.next))
            .chain(entities)
            .chain(streams)
            .chain(signals)
            .collect()
    }

    /// Applies one record: the single code path for live mutations and replay,
    /// so what is recovered is what was served.
    pub(crate) fn apply(&mut self, op: Op) {
        match op {
            Op::PutEntity(r) => {
                let key = r.descriptor.name.as_str().to_owned();
                self.bump(0, r.descriptor.id.get());
                self.entity_ids.insert(r.descriptor.id, key.clone());
                self.entities.insert(key, r);
            }
            Op::DeleteEntity(id) => {
                if let Some(key) = self.entity_ids.remove(&id) {
                    self.entities.remove(&key);
                }
            }
            Op::PutStream(r) => {
                let key = r.descriptor.name.as_str().to_owned();
                self.bump(2, r.descriptor.id.get());
                self.stream_ids.insert(r.descriptor.id, key.clone());
                self.streams.insert(key, r);
            }
            Op::DeleteStream(id) => {
                if let Some(key) = self.stream_ids.remove(&id) {
                    self.streams.remove(&key);
                }
            }
            Op::PutSignal(r) => self.put_signal(r),
            Op::DeleteSignal(id) => {
                if let Some(key) = self.signal_ids.remove(&id) {
                    self.routes.remove(&signal_name(&key.0, &key.1));
                    self.signals.remove(&key);
                }
            }
            // Replay unwraps batches; a marker never reaches the state.
            Op::Begin(_) => {}
            Op::NextIds(ids) => {
                for (slot, id) in ids.into_iter().enumerate() {
                    self.bump(slot, id.saturating_sub(1));
                }
            }
        }
    }

    fn put_signal(&mut self, r: SignalRecord) {
        let Some(entity) = self.entity_ids.get(&r.descriptor.entity).cloned() else {
            return;
        };
        let signal = r.descriptor.name.as_str().to_owned();
        self.bump(1, r.descriptor.id.get());
        self.signal_ids
            .insert(r.descriptor.id, (entity.clone(), signal.clone()));
        let name = signal_name(&entity, &signal);
        let held = self.routes.get(&name).map(|old| Arc::clone(&old.has_data));
        self.routes.insert(
            name,
            SignalRoute {
                id: r.descriptor.id,
                kind: r.descriptor.kind,
                stream: r.stream,
                has_data: held.unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
            },
        );
        self.signals.insert((entity, signal), r);
    }

    /// Keeps `next[slot]` above `used`.
    fn bump(&mut self, slot: usize, used: u64) {
        if let Some(n) = self.next.get_mut(slot) {
            *n = (*n).max(used.saturating_add(1));
        }
    }

    /// Takes the next ID of `slot`. The record carrying it advances the counter
    /// through [`State::apply`].
    pub(crate) fn peek_id(&self, slot: usize) -> u64 {
        self.next.get(slot).copied().unwrap_or(0).max(1)
    }
}
