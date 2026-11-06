//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which files a migration looks at, and those it discards to be rebuilt.

use std::fs;
use std::path::Path;

use norn_storage::{CLEAN_FILE, SegmentName, sync_dir};

use crate::error::{DatabaseError, Result};

/// The file a stream directory's journal lives under.
const JOURNAL: &str = "journal";

fn segments_in(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| DatabaseError::io("list", dir, e))? {
        let path = entry.map_err(|e| DatabaseError::io("list", dir, e))?.path();
        if SegmentName::parse(&path).is_some()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            out.push(format!("{prefix}/{name}"));
        }
    }
    Ok(())
}

/// Every source-of-truth file of the database, relative to `root`, in order: the
/// metadata log, each stream's segments and each journal's. Quarantine, the lock
/// files and anything a migration wrote are none of them. Lists; reads nothing.
pub(super) fn database_files(root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if root.join("metadata/metadata.log").exists() {
        out.push("metadata/metadata.log".to_owned());
    }
    let streams = root.join("streams");
    if !streams.exists() {
        return Ok(out);
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(&streams).map_err(|e| DatabaseError::io("list", &streams, e))? {
        let entry = entry.map_err(|e| DatabaseError::io("list", &streams, e))?;
        ids.push(entry.file_name().to_string_lossy().into_owned());
    }
    ids.sort();
    for id in ids {
        let dir = streams.join(&id);
        let mut found = Vec::new();
        segments_in(&dir, &format!("streams/{id}"), &mut found)?;
        segments_in(
            &dir.join(JOURNAL),
            &format!("streams/{id}/{JOURNAL}"),
            &mut found,
        )?;
        found.sort();
        out.extend(found);
    }
    Ok(out)
}

/// Removes what vouches for the old bytes and can be made again: every stream's clean
/// marker and the whole index. The next start reads and rebuilds them.
pub(super) fn drop_derived(root: &Path) -> Result<()> {
    let index = root.join("index");
    if index.exists() {
        fs::remove_dir_all(&index).map_err(|e| DatabaseError::io("remove", &index, e))?;
    }
    let streams = root.join("streams");
    if !streams.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&streams).map_err(|e| DatabaseError::io("list", &streams, e))? {
        let dir = entry
            .map_err(|e| DatabaseError::io("list", &streams, e))?
            .path();
        let marker = dir.join(CLEAN_FILE);
        if marker.exists() {
            fs::remove_file(&marker).map_err(|e| DatabaseError::io("remove", &marker, e))?;
            sync_dir(&dir)?;
        }
    }
    Ok(sync_dir(root)?)
}
