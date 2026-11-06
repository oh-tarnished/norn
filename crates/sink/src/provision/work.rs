//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What Norn has still to be told, in three queues: entities to create,
//! signals to create, and resources that exist but have changed. Entities
//! go before any signal, so a signal never races its parent and a batch of
//! signals is never cut short by an entity between them; an update never
//! holds up a create, because only a create holds readings back.
//!
//! Each queue is coalesced by name: a resource declared again while still
//! queued, as a signal whose name and unit arrive a second after its first
//! reading, replaces what was queued in place, so it is told once with what
//! is known by then.

use super::Resource;
use std::collections::{HashMap, VecDeque};

/// One queue, in order, one entry per name.
#[derive(Debug, Default)]
struct Queue {
    order: VecDeque<String>,
    items: HashMap<String, Resource>,
}

impl Queue {
    fn push(&mut self, resource: Resource) {
        let name = name(&resource).to_owned();
        if self.items.insert(name.clone(), resource).is_none() {
            self.order.push_back(name);
        }
    }

    fn push_front(&mut self, resource: Resource) {
        let name = name(&resource).to_owned();
        if self.items.insert(name.clone(), resource).is_none() {
            self.order.push_front(name);
        }
    }

    fn pop(&mut self) -> Option<Resource> {
        while let Some(name) = self.order.pop_front() {
            if let Some(resource) = self.items.remove(&name) {
                return Some(resource);
            }
        }
        None
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

/// A resource's full name.
pub(crate) fn name(resource: &Resource) -> &str {
    match resource {
        Resource::Entity(e) => &e.name,
        Resource::Signal { signal, .. } => &signal.name,
    }
}

/// The work of one sink. Plain data; behind the sink's one lock.
#[derive(Debug, Default)]
pub(crate) struct Work {
    entities: Queue,
    signals: Queue,
    updates: Queue,
}

/// Which queue a chunk came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Entities to create.
    Entities,
    /// Signals to create: their readings wait for them.
    Signals,
    /// Resources that exist, to bring up to date.
    Updates,
}

impl Work {
    /// Queues a resource to create (or to update, if it exists).
    pub(crate) fn push(&mut self, resource: Resource) {
        match resource {
            Resource::Entity(_) => self.entities.push(resource),
            Resource::Signal { .. } => self.signals.push(resource),
        }
    }

    /// Queues a resource that exists and has changed.
    pub(crate) fn update(&mut self, resource: Resource) {
        self.updates.push(resource);
    }

    /// The kind the next chunk is: the first queue with anything in it.
    #[must_use]
    pub(crate) fn next(&self) -> Option<Kind> {
        if self.entities.len() > 0 {
            Some(Kind::Entities)
        } else if self.signals.len() > 0 {
            Some(Kind::Signals)
        } else if self.updates.len() > 0 {
            Some(Kind::Updates)
        } else {
            None
        }
    }

    /// Up to `most` of the first queue with anything in it, entities first.
    pub(crate) fn take(&mut self, most: usize) -> Option<(Kind, Vec<Resource>)> {
        let kind = self.next()?;
        let queue = match kind {
            Kind::Entities => &mut self.entities,
            Kind::Signals => &mut self.signals,
            Kind::Updates => &mut self.updates,
        };
        let taken: Vec<Resource> = std::iter::from_fn(|| queue.pop()).take(most).collect();
        Some((kind, taken))
    }

    /// Puts a chunk back at the front of its queue, for the next try. A
    /// newer declaration queued meanwhile is kept over it.
    pub(crate) fn put_back(&mut self, kind: Kind, taken: Vec<Resource>) {
        let queue = match kind {
            Kind::Entities => &mut self.entities,
            Kind::Signals => &mut self.signals,
            Kind::Updates => &mut self.updates,
        };
        for resource in taken.into_iter().rev() {
            if !queue.items.contains_key(name(&resource)) {
                queue.push_front(resource);
            }
        }
    }

    /// Resources still to create: what readings may be waiting on.
    #[must_use]
    pub(crate) fn creates(&self) -> usize {
        self.entities.len() + self.signals.len()
    }

    /// Whether nothing at all is left.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.creates() + self.updates.len() == 0
    }
}

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
