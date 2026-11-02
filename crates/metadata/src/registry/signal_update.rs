//! Signals: update and delete.

use std::sync::PoisonError;

use norn_core::Timestamp;

use super::entity::check_revision;
use super::signal::not_found;
use super::spec::SignalPatch;
use super::state::signal_name;
use super::{Registry, Write};
use crate::error::{MetadataError, ResourceKind};
use crate::op::Op;
use crate::record::SignalRecord;
use crate::resource::check_display_name;

impl Registry {
    /// Changes a signal, if `expected` is unset or its current revision. Blocks
    /// on the disk.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`], [`MetadataError::RevisionMismatch`],
    /// [`MetadataError::Invalid`], or a log failure.
    pub fn update_signal(
        &self,
        entity: &str,
        id: &str,
        patch: SignalPatch,
        expected: Option<u64>,
        now: Timestamp,
        write: Write,
    ) -> Result<SignalRecord, MetadataError> {
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let key = (entity.to_owned(), id.to_owned());
        let current = st.signals.get(&key).ok_or_else(|| not_found(entity, id))?;
        let mut record = current.clone();
        check_revision(record.revision, expected)?;
        if let Some(name) = patch.display_name {
            check_display_name(&name)?;
            record.display_name = name;
        }
        if let Some(unit) = patch.unit {
            record.unit = unit;
        }
        if let Some(annotations) = patch.annotations {
            record.descriptor.metadata = annotations;
        }
        record.update_time = now;
        record.revision = record.revision.saturating_add(1);
        if write == Write::ValidateOnly {
            return Ok(record);
        }
        let op = Op::PutSignal(record.clone());
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)?;
        Ok(record)
    }

    /// Deletes a signal that holds no samples. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`], [`MetadataError::RevisionMismatch`],
    /// [`MetadataError::NotEmpty`], or a log failure.
    pub fn delete_signal(
        &self,
        entity: &str,
        id: &str,
        expected: Option<u64>,
        write: Write,
    ) -> Result<(), MetadataError> {
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let key = (entity.to_owned(), id.to_owned());
        let record = st.signals.get(&key).ok_or_else(|| not_found(entity, id))?;
        check_revision(record.revision, expected)?;
        let sid = record.descriptor.id;
        let route = st.routes.get(&signal_name(entity, id));
        if route.is_some_and(|r| r.holds_data()) {
            return Err(MetadataError::NotEmpty {
                kind: ResourceKind::Signal,
                id: format!("{entity}/{id}"),
                holds: "samples",
            });
        }
        if write == Write::ValidateOnly {
            return Ok(());
        }
        let op = Op::DeleteSignal(sid);
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)
    }
}
