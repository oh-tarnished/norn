//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Unix: open the directory read-only and fsync it.

use std::fs::{self, DirBuilder, File, Metadata, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

/// On macOS std's `sync_all` is `fcntl(F_FULLFSYNC)`, which also makes the drive
/// write its cache, so a directory entry survives power loss there too; see
/// `SegmentWriter::sync` for the trade-off.
pub(super) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

pub(super) fn exposed_mode(meta: &Metadata) -> Option<u32> {
    let mode = meta.permissions().mode() & 0o7777;
    (!meta.is_file() || mode & 0o077 != 0).then_some(mode)
}

pub(super) fn owner_only_dir(dir: &Path) -> io::Result<Option<u32>> {
    if !dir.exists() {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    let mode = fs::metadata(dir)?.permissions().mode() & 0o7777;
    if mode & 0o077 == 0 {
        return Ok(None);
    }
    fs::set_permissions(dir, Permissions::from_mode(mode & 0o700))?;
    Ok(Some(mode))
}
