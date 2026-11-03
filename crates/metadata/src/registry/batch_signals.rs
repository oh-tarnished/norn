//! Batch signal creates; the shared rules are in `batch`.

use std::collections::HashSet;
use std::sync::PoisonError;

use norn_core::{SignalDescriptor, SignalId, Timestamp};

use super::batch::{bounded, name, repeated};
use super::spec::SignalSpec;
use super::{Registry, Write, entity};
use crate::error::{MetadataError, ResourceKind};
use crate::op::Op;
use crate::record::SignalRecord;
use crate::resource::check_display_name;

impl Registry {
    /// Creates `signals`, given as (entity resource ID, signal resource ID,
    /// spec), in one durable batch at time `now`, and returns each one's record
    /// in the order given. A signal that already exists with the same value
    /// kind and stream is returned as stored. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::Invalid`] (also for a signal given twice),
    /// [`MetadataError::NotFound`] for an entity or a stream,
    /// [`MetadataError::AlreadyExists`] for an existing signal with another
    /// value kind or stream, [`MetadataError::LimitExceeded`] (`max_signals`,
    /// `max_batch_create`), or a log failure. On any error, nothing is created.
    pub fn create_signals(
        &self,
        signals: Vec<(String, String, SignalSpec)>,
        now: Timestamp,
        write: Write,
    ) -> Result<Vec<SignalRecord>, MetadataError> {
        bounded(signals.len())?;
        let st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let (mut seen, mut ops, mut out) = (HashSet::new(), Vec::new(), Vec::new());
        let mut next = st.peek_id(1);
        for (entity_id, id, spec) in signals {
            let name = name("signal_id", &id)?;
            check_display_name(&spec.display_name)?;
            let owner = st.entities.get(&entity_id);
            let entity = owner.ok_or_else(|| entity::not_found(&entity_id))?;
            let entity = entity.descriptor.id;
            let stream = st.streams.get(&spec.stream).ok_or_else(|| {
                let id = spec.stream.clone();
                let kind = ResourceKind::Stream;
                MetadataError::NotFound { kind, id }
            })?;
            let stream = stream.descriptor.id;
            let key = (entity_id, id);
            if !seen.insert(key.clone()) {
                return Err(repeated(
                    ResourceKind::Signal,
                    format!("{}/{}", key.0, key.1),
                ));
            }
            if let Some(existing) = st.signals.get(&key) {
                if existing.descriptor.kind != spec.kind || existing.stream != stream {
                    let id = format!("{}/{}", key.0, key.1);
                    let kind = ResourceKind::Signal;
                    return Err(MetadataError::AlreadyExists { kind, id });
                }
                out.push(existing.clone());
                continue;
            }
            let sid = SignalId::new(next);
            next = next.saturating_add(1);
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
            ops.push(Op::PutSignal(record.clone()));
            out.push(record);
        }
        if st.signals.len().saturating_add(ops.len()) > self.limits.max_signals {
            return Err(MetadataError::LimitExceeded {
                limit: "max_signals",
                max: self.limits.max_signals,
            });
        }
        self.write_batch(st, ops, write)?;
        Ok(out)
    }
}
