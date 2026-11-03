//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Streams: create, read, list, update, delete.

use std::ops::Bound;
use std::sync::PoisonError;

use norn_core::{Name, StreamDescriptor, StreamId, Timestamp};

use super::entity::check_revision;
use super::spec::{StreamPatch, StreamSpec};
use super::{Registry, Write};
use crate::error::{MetadataError, ResourceKind};
use crate::op::Op;
use crate::record::StreamRecord;
use crate::resource::{check_display_name, check_resource_id};

fn not_found(id: &str) -> MetadataError {
    MetadataError::NotFound {
        kind: ResourceKind::Stream,
        id: id.to_owned(),
    }
}

impl Registry {
    /// Creates a stream, durably, at time `now`. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::Invalid`], [`MetadataError::AlreadyExists`],
    /// [`MetadataError::LimitExceeded`] (`max_streams`), or a log failure.
    pub fn create_stream(
        &self,
        id: &str,
        spec: StreamSpec,
        now: Timestamp,
        write: Write,
    ) -> Result<StreamRecord, MetadataError> {
        check_resource_id("stream_id", id)?;
        check_display_name(&spec.display_name)?;
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if st.streams.contains_key(id) {
            return Err(MetadataError::AlreadyExists {
                kind: ResourceKind::Stream,
                id: id.to_owned(),
            });
        }
        if st.streams.len() >= self.limits.max_streams {
            return Err(MetadataError::LimitExceeded {
                limit: "max_streams",
                max: self.limits.max_streams,
            });
        }
        let name = Name::new(id).map_err(|e| MetadataError::Invalid {
            field: "stream_id",
            reason: e.to_string(),
        })?;
        let mut descriptor = StreamDescriptor::new(StreamId::new(st.peek_id(2)), name);
        descriptor.metadata = spec.annotations;
        let record = StreamRecord {
            descriptor,
            display_name: spec.display_name,
            retention: spec.retention,
            create_time: now,
            update_time: now,
            revision: 1,
        };
        if write == Write::ValidateOnly {
            self.refuse_if_poisoned()?;
            return Ok(record);
        }
        let op = Op::PutStream(record.clone());
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)?;
        Ok(record)
    }

    /// Reads one stream.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`].
    pub fn stream(&self, id: &str) -> Result<StreamRecord, MetadataError> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        st.streams.get(id).cloned().ok_or_else(|| not_found(id))
    }

    /// Up to `limit` streams after resource ID `after`, in name order, that
    /// satisfy `keep`.
    pub fn list_streams(
        &self,
        after: Option<&str>,
        limit: usize,
        keep: impl Fn(&StreamRecord) -> bool,
    ) -> Vec<StreamRecord> {
        let st = self.state.read().unwrap_or_else(PoisonError::into_inner);
        let from = after.map_or(Bound::Unbounded, Bound::Excluded);
        st.streams
            .range::<str, _>((from, Bound::Unbounded))
            .map(|(_, r)| r)
            .filter(|r| keep(r))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Changes a stream, if `expected` is unset or its current revision. Blocks
    /// on the disk.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`], [`MetadataError::RevisionMismatch`],
    /// [`MetadataError::Invalid`], or a log failure.
    pub fn update_stream(
        &self,
        id: &str,
        patch: StreamPatch,
        expected: Option<u64>,
        now: Timestamp,
        write: Write,
    ) -> Result<StreamRecord, MetadataError> {
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let mut record = st.streams.get(id).cloned().ok_or_else(|| not_found(id))?;
        check_revision(record.revision, expected)?;
        if let Some(name) = patch.display_name {
            check_display_name(&name)?;
            record.display_name = name;
        }
        if let Some(retention) = patch.retention {
            record.retention = retention;
        }
        if let Some(annotations) = patch.annotations {
            record.descriptor.metadata = annotations;
        }
        record.update_time = now;
        record.revision = record.revision.saturating_add(1);
        if write == Write::ValidateOnly {
            return Ok(record);
        }
        let op = Op::PutStream(record.clone());
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)?;
        Ok(record)
    }

    /// Deletes a stream no signal records into. A stream with no signals holds
    /// no samples, because a signal holding samples cannot be deleted. Its
    /// directory, if any, is left for an operator. Blocks on the disk.
    ///
    /// # Errors
    /// [`MetadataError::NotFound`], [`MetadataError::RevisionMismatch`],
    /// [`MetadataError::NotEmpty`], or a log failure.
    pub fn delete_stream(
        &self,
        id: &str,
        expected: Option<u64>,
        write: Write,
    ) -> Result<(), MetadataError> {
        let mut st = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let record = st.streams.get(id).ok_or_else(|| not_found(id))?;
        check_revision(record.revision, expected)?;
        let stream_id = record.descriptor.id;
        if st.signals.values().any(|s| s.stream == stream_id) {
            return Err(MetadataError::NotEmpty {
                kind: ResourceKind::Stream,
                id: id.to_owned(),
                holds: "signals",
            });
        }
        if write == Write::ValidateOnly {
            return Ok(());
        }
        let op = Op::DeleteStream(stream_id);
        let lsn = self.log_op(&op)?;
        st.apply(op);
        drop(st);
        self.commit_to(lsn)
    }
}
