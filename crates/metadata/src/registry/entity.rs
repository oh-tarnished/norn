//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Entities: create, read, list, update, delete.

use std::ops::Bound;
use std::sync::PoisonError;

use norn_core::{EntityDescriptor, EntityId, Name, Timestamp};

use super::spec::{EntityPatch, EntitySpec};
use super::{Registry, Write};
use crate::error::{MetadataError, ResourceKind};
use crate::op::Op;
use crate::record::EntityRecord;
use crate::resource::{check_display_name, check_resource_id};

pub(super) fn not_found(id: &str) -> MetadataError {
    MetadataError::NotFound {
        kind: ResourceKind::Entity,
        id: id.to_owned(),
    }
}

pub(super) fn check_revision(current: u64, expected: Option<u64>) -> Result<(), MetadataError> {
    match expected {
        Some(e) if e != current => Err(MetadataError::RevisionMismatch { current }),
        _ => Ok(()),
    }
}

impl Registry {
    /// Creates an entity, durably, at time `now`. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::Invalid`], [`MetadataError::AlreadyExists`],
    /// [`MetadataError::LimitExceeded`] (`max_entities`), or a log failure.
    pub fn create_entity(
        &self,
        id: &str,
        spec: EntitySpec,
        now: Timestamp,
        write: Write,
    ) -> Result<EntityRecord, MetadataError> {
        check_resource_id("entity_id", id)?;
        check_display_name(&spec.display_name)?;
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if st.entities.contains_key(id) {
            return Err(MetadataError::AlreadyExists {
                kind: ResourceKind::Entity,
                id: id.to_owned(),
            });
        }
        if st.entities.len() >= self.limits.max_entities {
            return Err(MetadataError::LimitExceeded {
                limit: "max_entities",
                max: self.limits.max_entities,
            });
        }
        let name = Name::new(id).map_err(|e| MetadataError::Invalid {
            field: "entity_id",
            reason: e.to_string(),
        })?;
        let mut descriptor = EntityDescriptor::new(EntityId::new(st.peek_id(0)), name);
        descriptor.metadata = spec.annotations;
        let record = EntityRecord {
            descriptor,
            display_name: spec.display_name,
            create_time: now,
            update_time: now,
            revision: 1,
        };
        if write == Write::ValidateOnly {
            self.refuse_if_poisoned()?;
            return Ok(record);
        }
        let op = Op::PutEntity(record.clone());
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)?;
        Ok(record)
    }

    /// Reads one entity.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`].
    pub fn entity(&self, id: &str) -> Result<EntityRecord, MetadataError> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        st.entities.get(id).cloned().ok_or_else(|| not_found(id))
    }

    /// Up to `limit` entities after resource ID `after`, in name order, that
    /// satisfy `keep`. Scans at most every entity, under a shared lock.
    pub fn list_entities(
        &self,
        after: Option<&str>,
        limit: usize,
        keep: impl Fn(&EntityRecord) -> bool,
    ) -> Vec<EntityRecord> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        let from = after.map_or(Bound::Unbounded, Bound::Excluded);
        st.entities
            .range::<str, _>((from, Bound::Unbounded))
            .map(|(_, r)| r)
            .filter(|r| keep(r))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Changes an entity, if `expected` is unset or its current revision.
    /// Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`], [`MetadataError::RevisionMismatch`],
    /// [`MetadataError::Invalid`], or a log failure.
    pub fn update_entity(
        &self,
        id: &str,
        patch: EntityPatch,
        expected: Option<u64>,
        now: Timestamp,
        write: Write,
    ) -> Result<EntityRecord, MetadataError> {
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let mut record = st.entities.get(id).cloned().ok_or_else(|| not_found(id))?;
        check_revision(record.revision, expected)?;
        if let Some(name) = patch.display_name {
            check_display_name(&name)?;
            record.display_name = name;
        }
        if let Some(annotations) = patch.annotations {
            record.descriptor.metadata = annotations;
        }
        record.update_time = now;
        record.revision = record.revision.saturating_add(1);
        if write == Write::ValidateOnly {
            return Ok(record);
        }
        let op = Op::PutEntity(record.clone());
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)?;
        Ok(record)
    }

    /// Deletes an entity with no signals. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`], [`MetadataError::RevisionMismatch`],
    /// [`MetadataError::NotEmpty`], or a log failure.
    pub fn delete_entity(
        &self,
        id: &str,
        expected: Option<u64>,
        write: Write,
    ) -> Result<(), MetadataError> {
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let record = st.entities.get(id).ok_or_else(|| not_found(id))?;
        check_revision(record.revision, expected)?;
        let entity_id = record.descriptor.id;
        let owned = id.to_owned();
        let children = (owned.clone(), String::new())..;
        if st
            .signals
            .range(children)
            .next()
            .is_some_and(|((e, _), _)| *e == owned)
        {
            return Err(MetadataError::NotEmpty {
                kind: ResourceKind::Entity,
                id: owned,
                holds: "signals",
            });
        }
        if write == Write::ValidateOnly {
            return Ok(());
        }
        let op = Op::DeleteEntity(entity_id);
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)
    }
}
