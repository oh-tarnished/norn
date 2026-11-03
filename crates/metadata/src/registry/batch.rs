//! Batch creates: many resources, one log write, one fsync, all or nothing.
//!
//! A site provisions tens of thousands of signals at start. One create per
//! call costs one call and one shared fsync each; a batch costs one of each
//! for up to [`MAX_BATCH_CREATE`] resources, and the log's batch marker makes a
//! crash keep all of it or none (`crate::log`).
//!
//! A batch is safe to repeat, which is what a client restarting its
//! provisioning needs: a resource that already exists is returned as stored,
//! its metadata left alone, rather than failing the batch. A signal must still
//! agree on what can never change, its value kind and stream
//! (`batch_signals`).

use std::collections::HashSet;
use std::sync::{PoisonError, RwLockWriteGuard};

use norn_core::{EntityDescriptor, EntityId, Name, Timestamp};

use super::spec::EntitySpec;
use super::state::State;
use super::{Registry, Write};
use crate::error::{MetadataError, ResourceKind};
use crate::op::Op;
use crate::record::EntityRecord;
use crate::resource::{check_display_name, check_resource_id};

/// The most resources one batch create may hold. Bounds the memory a batch
/// holds under the state lock, and what replay holds before applying it.
pub const MAX_BATCH_CREATE: usize = 1_000;

pub(super) fn name(field: &'static str, id: &str) -> Result<Name, MetadataError> {
    check_resource_id(field, id)?;
    Name::new(id).map_err(|e| MetadataError::Invalid {
        field,
        reason: e.to_string(),
    })
}

pub(super) fn bounded(n: usize) -> Result<(), MetadataError> {
    if n > MAX_BATCH_CREATE {
        return Err(MetadataError::LimitExceeded {
            limit: "max_batch_create",
            max: MAX_BATCH_CREATE,
        });
    }
    Ok(())
}

pub(super) fn repeated(kind: ResourceKind, id: String) -> MetadataError {
    MetadataError::Invalid {
        field: "requests",
        reason: format!("{kind:?} {id} appears more than once"),
    }
}

impl Registry {
    /// Creates `entities`, given as (resource ID, spec), in one durable batch
    /// at time `now`, and returns each one's record in the order given. An
    /// entity that already exists is returned as stored. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::Invalid`] (also for an ID given twice),
    /// [`MetadataError::LimitExceeded`] (`max_entities`, `max_batch_create`),
    /// or a log failure. On any error, nothing is created.
    pub fn create_entities(
        &self,
        entities: Vec<(String, EntitySpec)>,
        now: Timestamp,
        write: Write,
    ) -> Result<Vec<EntityRecord>, MetadataError> {
        bounded(entities.len())?;
        let st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let (mut seen, mut ops, mut out) = (HashSet::new(), Vec::new(), Vec::new());
        let mut next = st.peek_id(0);
        for (id, spec) in entities {
            let name = name("entity_id", &id)?;
            check_display_name(&spec.display_name)?;
            if !seen.insert(id.clone()) {
                return Err(repeated(ResourceKind::Entity, id));
            }
            if let Some(existing) = st.entities.get(&id) {
                out.push(existing.clone());
                continue;
            }
            let mut descriptor = EntityDescriptor::new(EntityId::new(next), name);
            next = next.saturating_add(1);
            descriptor.metadata = spec.annotations;
            let record = EntityRecord {
                descriptor,
                display_name: spec.display_name,
                create_time: now,
                update_time: now,
                revision: 1,
            };
            ops.push(Op::PutEntity(record.clone()));
            out.push(record);
        }
        if st.entities.len().saturating_add(ops.len()) > self.limits.max_entities {
            return Err(MetadataError::LimitExceeded {
                limit: "max_entities",
                max: self.limits.max_entities,
            });
        }
        self.write_batch(st, ops, write)?;
        Ok(out)
    }

    /// Logs `ops` as one batch, applies them, releases `st`, and waits until
    /// they are durable. With nothing new, still waits until whatever the
    /// caller was shown is durable, since a record is visible before it is.
    pub(super) fn write_batch(
        &self,
        mut st: RwLockWriteGuard<'_, State>,
        ops: Vec<Op>,
        write: Write,
    ) -> Result<(), MetadataError> {
        if write == Write::ValidateOnly {
            return self.refuse_if_poisoned();
        }
        if ops.is_empty() {
            drop(st);
            return self.commit();
        }
        let lsn = self.log_ops(&ops)?;
        ops.into_iter().for_each(|op| st.apply(op));
        drop(st);
        self.commit_to(lsn)
    }
}
