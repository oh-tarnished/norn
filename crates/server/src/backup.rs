//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `AdminService.CreateBackup`: refusing a bad request now, and starting the job.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use norn_core::StreamId;
use norn_protocol::admin::CreateBackupRequest;
use norn_protocol::common::ErrorReason;
use norn_protocol::google::longrunning::Operation;
use norn_runtime::maintenance::{BackupSpec, Maintenance, Request};
use tonic::{Code, Status};

use crate::operations::{refused, to_wire};
use crate::status::{bad_field, error_info};

/// The longest backup ID: as long as a stream's.
const MAX_ID: usize = 63;

fn id_is_valid(id: &str) -> bool {
    id.len() <= MAX_ID
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn absolute(field: &str, path: &str) -> Result<PathBuf, Status> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(bad_field(
            field,
            "must be an absolute path on the daemon's machine",
        ))
    }
}

fn not_empty(path: &Path) -> Status {
    let shown = path.display().to_string();
    error_info(
        Code::FailedPrecondition,
        ErrorReason::ResourceNotEmpty,
        format!("{shown} is not empty: a backup needs a directory of its own"),
        [("path", shown.clone())],
    )
}

/// Checks `request` and accepts the job. One directory listing; never waits for it.
/// The client's paths are on the daemon's machine and the socket admits only the
/// daemon's own user, so this writes nowhere that client could not write itself.
pub(crate) fn start(
    maintenance: &Arc<Maintenance>,
    request: CreateBackupRequest,
) -> Result<Operation, Status> {
    if !id_is_valid(&request.backup_id) {
        let rule = "must be 1 to 63 characters of [a-z0-9-], starting with a letter";
        return Err(bad_field("backup_id", rule));
    }
    let backup = request
        .backup
        .ok_or_else(|| bad_field("backup", "is required"))?;
    let to = absolute("backup.path", &backup.path)?;
    let base = match backup.base_path.as_str() {
        "" => None,
        path => Some(absolute("backup.base_path", path)?),
    };
    let vacant = norn_database::is_vacant(&to).map_err(|e| Status::internal(e.to_string()))?;
    if !vacant {
        return Err(not_empty(&to));
    }
    let spec = BackupSpec::new(to, base, request.backup_id);
    let job = Request::Backup(spec);
    let op = maintenance.submit(StreamId::new(0), job).map_err(refused)?;
    Ok(to_wire(&op))
}
