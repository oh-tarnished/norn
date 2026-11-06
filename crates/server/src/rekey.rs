//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn rekey`: every stored segment rewritten under the current key, with the
//! daemon stopped. `nornctl rekey` runs this, since nornctl reaches a database
//! only through the protocol (the arch gate), and this rewrites files.

use norn_runtime::Runtime;
use norn_runtime::storage::{
    EncryptionError, RekeyReport, StorageError, StorageOptions, rekey_database,
};

use crate::config::DaemonConfig;
use crate::daemon::DaemonError;
use crate::daemon::open::open_database as open_for_rekey;

/// Recovers the database and rewrites what is under an older key. Holds the
/// registry's lock throughout, so it fails while a daemon has the database.
///
/// # Errors
/// [`DaemonError::Database`] with [`EncryptionError::Mismatch`] when encryption is
/// off, a wrong key, or anything recovery or rewriting fails on;
/// [`DaemonError::Metadata`] when a daemon holds the database.
pub async fn rekey(config: &DaemonConfig) -> Result<RekeyReport, DaemonError> {
    let Some(source) = config.key_source() else {
        let path = config.database();
        let off = EncryptionError::Mismatch {
            path,
            encrypted: false,
        };
        return Err(DaemonError::Database(StorageError::Encryption(off)));
    };
    let keys = source.load().map_err(StorageError::Encryption);
    let keys = keys.map_err(DaemonError::Database)?.leak();
    let runtime = Runtime::new(*config.engine())?;
    let database = open_for_rekey(config, &runtime).await?;
    let root = config.database();
    let mut opts = StorageOptions::from_config(config.engine());
    opts.keyring = Some(keys);
    let done = tokio::task::spawn_blocking(move || rekey_database(&root, &opts)).await;
    drop(database);
    done.map_err(|_| DaemonError::RecoveryPanicked)?
        .map_err(DaemonError::Database)
}
