//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Opening the database: its directory, its keys, its registry, and recovery.

use std::sync::Arc;

use norn_metadata::Registry;
use norn_runtime::ingest::{Ingest, IngestRecovery};
use norn_runtime::storage::{StorageError, check_database, secure_database_dir};
use norn_runtime::{Runtime, Subsystem};

use super::DaemonError;
use crate::config::DaemonConfig;

/// The recovered database.
pub(crate) struct Database {
    pub(super) registry: Arc<Registry>,
    pub(super) ingest: Arc<Ingest>,
    pub(super) recovery: IngestRecovery,
}

/// Opens the registry and recovers every stream, on the blocking pool.
pub(crate) async fn open_database(
    config: &DaemonConfig,
    runtime: &Runtime,
) -> Result<Database, DaemonError> {
    let engine = *config.engine();
    let clock = Arc::clone(runtime.clock());
    let budget = runtime.budgets().get(Subsystem::Ingest).clone();
    let root = config.database();
    let source = config.key_source().cloned();
    let database = tokio::task::spawn_blocking(move || {
        if let Some(mode) = secure_database_dir(&root).map_err(DaemonError::Database)? {
            tracing::warn!(
                mode = format!("{mode:04o}"),
                "the database directory was open to other users; it is now 0700"
            );
        }
        let keys = match source {
            Some(source) => {
                let keys = source.load().map_err(StorageError::Encryption);
                Some(keys.map_err(DaemonError::Database)?.leak())
            }
            None => None,
        };
        // Before anything recovery would repair: a wrong key changes nothing.
        check_database(&root, keys).map_err(DaemonError::Database)?;
        norn_database::check_openable(&root).map_err(DaemonError::StoreVersion)?;
        let dir = root.join("metadata");
        let (registry, log) = Registry::open(&dir, engine.limits(), engine.metadata())?;
        norn_database::stamp(&root).map_err(DaemonError::StoreVersion)?;
        tracing::info!(
            records = log.records,
            torn = log.torn_bytes,
            "registry open"
        );
        let registry = Arc::new(registry);
        let shared = Arc::clone(&registry);
        let ingest = Ingest::new(root, &engine, shared, budget, clock).keyring(keys);
        let recovery = ingest.recover().map_err(DaemonError::Storage)?;
        let ingest = Arc::new(ingest);
        Ok::<_, DaemonError>(Database {
            registry,
            ingest,
            recovery,
        })
    })
    .await
    .map_err(|_| DaemonError::RecoveryPanicked)??;
    let recovery = &database.recovery;
    if recovery.corruptions > 0 {
        tracing::warn!(?recovery, "recovery found damaged data");
    } else {
        tracing::info!(?recovery, "recovery finished");
    }
    Ok(database)
}
