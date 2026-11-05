//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `EntityService`, `SignalService` and `StreamService` over the registry.

mod batch;
mod entity;
pub(crate) mod names;
mod query;
mod segments;
pub(crate) mod select;
mod signal;
mod stream;
mod upkeep;

use std::collections::HashMap;
use std::sync::Arc;

use norn_core::Metadata;
use norn_metadata::{MetadataError, Registry, Write};
use norn_protocol::common::ErrorReason;
use norn_runtime::Clock;
use norn_runtime::ingest::Ingest;
use norn_runtime::maintenance::Maintenance;
use tonic::{Code, Status};

use crate::status::{bad_field, error_info};

/// The catalogue services: one type, three traits, one registry.
///
/// Reads are served on the async thread, since they touch only memory. Mutations
/// fsync, so they run on tokio's blocking pool; the pool's bound is therefore
/// what limits concurrent mutations. Concurrency: `Clone`, shared by tonic.
#[derive(Debug, Clone)]
pub struct Catalog {
    registry: Arc<Registry>,
    ingest: Arc<Ingest>,
    clock: Arc<dyn Clock>,
    maintenance: Arc<Maintenance>,
}

impl Catalog {
    /// A catalogue over `registry`, reading stream totals from `ingest` and
    /// handing long-running work to `maintenance`.
    #[must_use]
    pub fn new(
        registry: Arc<Registry>,
        ingest: Arc<Ingest>,
        clock: Arc<dyn Clock>,
        maintenance: Arc<Maintenance>,
    ) -> Self {
        Self {
            registry,
            ingest,
            clock,
            maintenance,
        }
    }

    /// Runs a mutation off the async thread.
    async fn mutate<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Registry) -> Result<T, MetadataError> + Send + 'static,
    ) -> Result<T, Status> {
        let registry = Arc::clone(&self.registry);
        tokio::task::spawn_blocking(move || work(&registry))
            .await
            .map_err(|_| Status::internal("a registry mutation panicked"))?
            .map_err(|e| metadata_status(&e))
    }
}

/// The write mode for a request's `validate_only`.
fn write_mode(validate_only: bool) -> Write {
    if validate_only {
        Write::ValidateOnly
    } else {
        Write::Commit
    }
}

/// An etag from a revision, and back. An unparseable etag can match nothing, so
/// it reads as a revision no resource has.
fn etag(revision: u64) -> String {
    format!("\"{revision}\"")
}

fn expected(etag: &str) -> Option<u64> {
    if etag.is_empty() {
        return None;
    }
    Some(etag.trim_matches('"').parse().unwrap_or(u64::MAX))
}

/// `annotations` as bounded metadata.
fn annotations(registry: &Registry, map: HashMap<String, String>) -> Result<Metadata, Status> {
    let mut out = Metadata::with_limits(registry.metadata_limits());
    for (k, v) in map {
        out.insert(k, v)
            .map_err(|e| bad_field("annotations", &e.to_string()))?;
    }
    Ok(out)
}

fn annotation_map(metadata: &Metadata) -> HashMap<String, String> {
    metadata
        .iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// A registry failure as the protocol's status, always with one `ErrorInfo`.
pub(crate) fn metadata_status(e: &MetadataError) -> Status {
    let text = e.to_string();
    match e {
        MetadataError::NotFound { id, .. } => {
            let meta = [("resource", id.clone())];
            error_info(Code::NotFound, ErrorReason::ResourceNotFound, text, meta)
        }
        MetadataError::AlreadyExists { id, .. } => {
            let meta = [("resource", id.clone())];
            let reason = ErrorReason::ResourceAlreadyExists;
            error_info(Code::AlreadyExists, reason, text, meta)
        }
        MetadataError::Invalid { field, reason } => bad_field(field, reason),
        MetadataError::RevisionMismatch { .. } => {
            error_info(Code::Aborted, ErrorReason::EtagMismatch, text, [])
        }
        MetadataError::NotEmpty { id, holds, .. } => {
            let meta = [("resource", id.clone()), ("holds", (*holds).to_owned())];
            let reason = ErrorReason::ResourceNotEmpty;
            error_info(Code::FailedPrecondition, reason, text, meta)
        }
        MetadataError::LimitExceeded { limit, .. } => {
            let meta = [("limit", (*limit).to_owned())];
            error_info(
                Code::ResourceExhausted,
                ErrorReason::CapacityExhausted,
                text,
                meta,
            )
        }
        _ => error_info(Code::Internal, ErrorReason::StorageUnavailable, text, []),
    }
}
