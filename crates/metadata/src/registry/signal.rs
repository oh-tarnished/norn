//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Signals: create, read, list, update, delete.

use std::ops::Bound;
use std::sync::PoisonError;

use norn_core::{Name, SignalDescriptor, SignalId, Timestamp};

use super::entity;
use super::spec::SignalSpec;
use super::{Registry, Write};
use crate::error::{MetadataError, ResourceKind};
use crate::op::Op;
use crate::record::SignalRecord;
use crate::resource::{check_display_name, check_resource_id};

pub(super) fn not_found(entity: &str, signal: &str) -> MetadataError {
    MetadataError::NotFound {
        kind: ResourceKind::Signal,
        id: format!("{entity}/{signal}"),
    }
}

impl Registry {
    /// Creates a signal under `entity`, durably. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::Invalid`], [`MetadataError::AlreadyExists`],
    /// [`MetadataError::NotFound`] for the entity or the stream,
    /// [`MetadataError::LimitExceeded`] (`max_signals`), or a log failure.
    pub fn create_signal(
        &self,
        entity_id: &str,
        id: &str,
        spec: SignalSpec,
        now: Timestamp,
        write: Write,
    ) -> Result<SignalRecord, MetadataError> {
        check_resource_id("signal_id", id)?;
        check_display_name(&spec.display_name)?;
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let entity = st
            .entities
            .get(entity_id)
            .ok_or_else(|| entity::not_found(entity_id))?
            .descriptor
            .id;
        let stream = st
            .streams
            .get(&spec.stream)
            .ok_or_else(|| MetadataError::NotFound {
                kind: ResourceKind::Stream,
                id: spec.stream.clone(),
            })?
            .descriptor
            .id;
        let key = (entity_id.to_owned(), id.to_owned());
        if st.signals.contains_key(&key) {
            return Err(MetadataError::AlreadyExists {
                kind: ResourceKind::Signal,
                id: format!("{entity_id}/{id}"),
            });
        }
        if st.signals.len() >= self.limits.max_signals {
            return Err(MetadataError::LimitExceeded {
                limit: "max_signals",
                max: self.limits.max_signals,
            });
        }
        let name = Name::new(id).map_err(|e| MetadataError::Invalid {
            field: "signal_id",
            reason: e.to_string(),
        })?;
        let sid = SignalId::new(st.peek_id(1));
        let mut descriptor = SignalDescriptor::new(sid, entity, name, spec.kind);
        descriptor.metadata = spec.annotations;
        let record = SignalRecord {
            descriptor,
            stream,
            display_name: spec.display_name,
            unit: spec.unit,
            create_time: now,
            update_time: now,
            revision: 1,
        };
        if write == Write::ValidateOnly {
            self.refuse_if_poisoned()?;
            return Ok(record);
        }
        let op = Op::PutSignal(record.clone());
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)?;
        Ok(record)
    }

    /// Reads one signal.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`].
    pub fn signal(&self, entity: &str, id: &str) -> Result<SignalRecord, MetadataError> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        let key = (entity.to_owned(), id.to_owned());
        st.signals
            .get(&key)
            .cloned()
            .ok_or_else(|| not_found(entity, id))
    }

    /// Up to `limit` signals after `(entity, signal)` `after`, in name order,
    /// with their entity's resource ID. `entity` limits it to one entity; `None`
    /// is the wildcard parent `entities/-`.
    pub fn list_signals(
        &self,
        entity: Option<&str>,
        after: Option<(String, String)>,
        limit: usize,
        keep: impl Fn(&SignalRecord) -> bool,
    ) -> Vec<(String, SignalRecord)> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        let from = after.map_or(Bound::Unbounded, Bound::Excluded);
        st.signals
            .range((from, Bound::Unbounded))
            .filter(|((e, _), _)| entity.is_none_or(|want| want == e))
            .map(|((e, _), r)| (e, r))
            .filter(|(_, r)| keep(r))
            .take(limit)
            .map(|(e, r)| (e.clone(), r.clone()))
            .collect()
    }
}
