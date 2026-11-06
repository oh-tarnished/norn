//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What readings need to exist: entities, and signals of them. An adapter maps
//! its protocol's things onto these once, and its protocol's own vocabulary
//! (addresses, object kinds, vendor names) into annotations, which Norn
//! indexes for filtering and never interprets (SPEC §11, §30).

use crate::ids;
use crate::provision::Resource;
use norn_protocol::common::ValueType;
use std::collections::HashMap;

/// An entity to declare: a thing that exists. Required fields positional,
/// the rest builder methods, so adding an option never breaks a caller.
///
/// Plain data: `Send`, `Sync`, `Clone`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) annotations: HashMap<String, String>,
}

impl Entity {
    /// The entity `id`, checked at [`Sink::declare`](crate::Sink::declare).
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: String::new(),
            annotations: HashMap::new(),
        }
    }

    /// What a person sees.
    #[must_use]
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// One annotation, replacing any of the same key.
    #[must_use]
    pub fn annotation(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    pub(crate) fn resource(&self) -> Resource {
        Resource::Entity(norn_protocol::entity::Entity {
            name: ids::entity_name(&self.id),
            display_name: self.display_name.clone(),
            annotations: self.annotations.clone(),
            ..Default::default()
        })
    }
}

/// A signal to declare: one measurable property of an entity, of one value
/// type for life. Plain data: `Send`, `Sync`, `Clone`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    pub(crate) entity: String,
    pub(crate) id: String,
    pub(crate) value_type: ValueType,
    pub(crate) display_name: String,
    pub(crate) unit: String,
    pub(crate) annotations: HashMap<String, String>,
}

impl Signal {
    /// The signal `id` of `entity`, holding values of `value_type`.
    #[must_use]
    pub fn new(entity: impl Into<String>, id: impl Into<String>, value_type: ValueType) -> Self {
        Self {
            entity: entity.into(),
            id: id.into(),
            value_type,
            display_name: String::new(),
            unit: String::new(),
            annotations: HashMap::new(),
        }
    }

    /// What a person sees.
    #[must_use]
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// The unit of its values, as free text (`degree_celsius`, `rpm`).
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// One annotation, replacing any of the same key.
    #[must_use]
    pub fn annotation(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    /// `entities/{entity}/signals/{id}`.
    #[must_use]
    pub fn name(&self) -> String {
        ids::signal_name(&self.entity, &self.id)
    }

    pub(crate) fn resource(&self, stream: &str) -> Resource {
        Resource::Signal {
            parent: ids::entity_name(&self.entity),
            id: self.id.clone(),
            signal: norn_protocol::signal::Signal {
                name: self.name(),
                display_name: self.display_name.clone(),
                value_type: self.value_type as i32,
                stream: stream.to_owned(),
                unit: self.unit.clone(),
                annotations: self.annotations.clone(),
                ..Default::default()
            },
        }
    }
}

/// Entities and signals, declared together. Declaring is incremental and
/// idempotent: a catalog may hold only what discovery found since the last,
/// or everything again; what is unchanged costs nothing.
///
/// Plain data: `Send`, `Sync`, `Clone`. [`Default`] is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub(crate) entities: Vec<Entity>,
    pub(crate) signals: Vec<Signal>,
}

impl Catalog {
    /// An empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `entity`.
    #[must_use]
    pub fn entity(mut self, entity: Entity) -> Self {
        self.entities.push(entity);
        self
    }

    /// Adds `signal`.
    #[must_use]
    pub fn signal(mut self, signal: Signal) -> Self {
        self.signals.push(signal);
        self
    }

    /// Adds `entity`, in place, for a catalog built in a loop.
    pub fn push_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
    }

    /// Adds `signal`, in place.
    pub fn push_signal(&mut self, signal: Signal) {
        self.signals.push(signal);
    }

    /// Whether it declares nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.signals.is_empty()
    }
}
