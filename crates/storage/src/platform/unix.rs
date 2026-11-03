//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Unix: open the directory read-only and fsync it.

use std::fs::File;
use std::io;
use std::path::Path;

/// On macOS std's `sync_all` is `fcntl(F_FULLFSYNC)`, which also makes the drive
/// write its cache, so a directory entry survives power loss there too; see
/// `SegmentWriter::sync` for the trade-off.
pub(super) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}
