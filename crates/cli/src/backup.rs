//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl backup`: a long-running operation on the daemon, polled.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Backup, CreateBackupRequest};
use tonic::transport::Channel;

use crate::CliError;
use crate::resources::{call, usage};
use crate::time::format;
use crate::upkeep::finish;

const USAGE: &str = "backup --to DIR [--incremental-from BACKUP] [--id ID]";

/// `path` as the daemon needs it: absolute, whichever directory `nornctl` ran in.
fn absolute(path: &str) -> Result<String, CliError> {
    let path = Path::new(path);
    let full: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(full.display().to_string())
}

fn default_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    format!("backup-{}", now.map_or(0, |d| d.as_secs()))
}

/// Parses the words, starts the backup and waits for it: each poll is one call, so
/// `--timeout` bounds a poll and not the backup.
pub(crate) async fn backup(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let (mut to, mut base, mut id) = (None, None, None);
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        let slot = match *word {
            "--to" => &mut to,
            "--incremental-from" => &mut base,
            "--id" => &mut id,
            _ => return Err(usage(USAGE)),
        };
        *slot = Some(*rest.next().ok_or_else(|| usage(USAGE))?);
    }
    let to = to.ok_or_else(|| usage(USAGE))?;
    let request = CreateBackupRequest {
        backup_id: id.map_or_else(default_id, str::to_owned),
        backup: Some(Backup {
            path: absolute(to)?,
            base_path: base.map(absolute).transpose()?.unwrap_or_default(),
            ..Backup::default()
        }),
    };
    let mut client = AdminServiceClient::new(channel.clone());
    let op = client.create_backup(request).await;
    let op = op.map_err(call("AdminService.CreateBackup"))?.into_inner();
    let r: Backup = finish(channel, op).await?;
    let cut = r
        .cut_time
        .map_or("-".into(), |t| format(t.seconds, t.nanos));
    writeln!(
        out,
        "{} cut={cut} streams={} files={} bytes={} copied_files={} copied_bytes={}",
        r.path,
        r.stream_count,
        r.file_count,
        r.byte_count,
        r.copied_file_count,
        r.copied_byte_count
    )?;
    Ok(())
}
