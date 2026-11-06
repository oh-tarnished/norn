//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a backup reports, and how it names the backup it builds on.

use std::fs;
use std::path::Path;

use crate::error::{DatabaseError, Result};

/// What a finished backup holds.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupSummary {
    /// Files the backup lists, however many it copied.
    pub files: u64,
    /// Bytes those files take.
    pub bytes: u64,
    /// Files copied by this backup.
    pub copied_files: u64,
    /// Bytes copied by this backup.
    pub copied_bytes: u64,
}

/// `base` as a path a backup in `dir` can name: `../name` for a sibling, so the two
/// can move together, and absolute otherwise.
pub(super) fn parent_path(dir: &Path, base: &Path) -> Result<String> {
    let io = |e, p: &Path| DatabaseError::io("resolve", p, e);
    let full = fs::canonicalize(base).map_err(|e| io(e, base))?;
    let here = fs::canonicalize(dir).map_err(|e| io(e, dir))?;
    let name = full.file_name().and_then(|n| n.to_str());
    if let Some(name) = name.filter(|_| full.parent() == here.parent()) {
        return Ok(format!("../{name}"));
    }
    full.to_str()
        .map(str::to_owned)
        .ok_or_else(|| DatabaseError::corrupt(base, "a backup's path must be UTF-8 to be recorded"))
}
