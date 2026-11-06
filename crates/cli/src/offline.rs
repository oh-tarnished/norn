//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The commands with no daemon to ask: `restore` and `migrate` work on files, and hold
//! the lock a daemon would hold, so they and a daemon exclude each other.

use std::io::Write;
use std::path::Path;

use norn_database::{MigrateOptions, check_backup, migrate, restore};

use crate::CliError;

const RESTORE: &str = "restore --from DIR --data-dir D [--dry-run]";
const MIGRATE: &str = "migrate --data-dir D --to VERSION [--dry-run] [--keep-old]";

fn refused(e: norn_database::DatabaseError) -> CliError {
    CliError::Refused(e.to_string())
}

/// Runs one offline command, whose words start with its name. Blocks on the disk.
///
/// # Errors
/// [`CliError::Usage`] for malformed words; [`CliError::Refused`] for what the job
/// reports, which says what to do next.
pub fn run(words: &[String], out: &mut impl Write) -> Result<(), CliError> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["restore", rest @ ..] => run_restore(rest, out),
        ["migrate", rest @ ..] => run_migrate(rest, out),
        _ => Err(CliError::Usage(format!("usage: nornctl {RESTORE}"))),
    }
}

fn run_restore(words: &[&str], out: &mut impl Write) -> Result<(), CliError> {
    let usage = || CliError::Usage(format!("usage: nornctl {RESTORE}"));
    let (mut from, mut data_dir, mut dry_run) = (None, None, false);
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match *word {
            "--from" => from = Some(*rest.next().ok_or_else(usage)?),
            "--data-dir" => data_dir = Some(*rest.next().ok_or_else(usage)?),
            "--dry-run" => dry_run = true,
            _ => return Err(usage()),
        }
    }
    let from = Path::new(from.ok_or_else(usage)?);
    if dry_run {
        let c = check_backup(from).map_err(refused)?;
        writeln!(
            out,
            "backup intact files={} bytes={} chain={}",
            c.files, c.bytes, c.chain
        )?;
        return Ok(());
    }
    let root = Path::new(data_dir.ok_or_else(usage)?);
    let r = restore(from, root).map_err(refused)?;
    writeln!(
        out,
        "restored {} files={} bytes={} already_done={} records={} verified_segments={} \
         damaged_regions={}",
        root.display(),
        r.files,
        r.bytes,
        r.already_done,
        r.records,
        r.verified_segments,
        r.damaged_regions
    )?;
    if r.damaged_regions > 0 {
        return Err(CliError::Refused(format!(
            "{}: the restored database has {} damaged regions, as it had when backed up; \
             the rest is whole and the daemon will serve it, and report them",
            root.display(),
            r.damaged_regions
        )));
    }
    Ok(())
}

fn run_migrate(words: &[&str], out: &mut impl Write) -> Result<(), CliError> {
    let usage = || CliError::Usage(format!("usage: nornctl {MIGRATE}"));
    let (mut data_dir, mut to, mut dry_run, mut keep_old) = (None, None, false, false);
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match *word {
            "--data-dir" => data_dir = Some(*rest.next().ok_or_else(usage)?),
            "--to" => {
                to = Some(
                    rest.next()
                        .ok_or_else(usage)?
                        .parse()
                        .map_err(|_| usage())?,
                )
            }
            "--dry-run" => dry_run = true,
            "--keep-old" => keep_old = true,
            _ => return Err(usage()),
        }
    }
    let root = Path::new(data_dir.ok_or_else(usage)?);
    let options = MigrateOptions {
        to: to.ok_or_else(usage)?,
        dry_run,
        keep_old,
    };
    let r = migrate(root, options).map_err(refused)?;
    let verb = if dry_run { "would migrate" } else { "migrated" };
    writeln!(
        out,
        "{verb} {} store_version={}->{} steps={} files={} resumed={} verified_segments={} \
         damaged_regions={}",
        root.display(),
        r.from,
        r.to,
        r.steps,
        r.files,
        r.resumed,
        r.verified_segments,
        r.damaged_regions
    )?;
    Ok(())
}
