//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a stopped database through, changing nothing: what `verify` would say.

use std::fs;
use std::path::Path;

use norn_core::Config;
use norn_metadata::inspect_log;
use norn_storage::{RecoveryMode, StorageOptions, recover_stream};

use crate::error::{DatabaseError, Result};
use crate::lock::METADATA_DIR;

/// What reading a database found.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Inspection {
    /// Metadata records that replay.
    pub records: u64,
    /// Segments whose every chunk was read and checked.
    pub verified_segments: u64,
    /// Damaged regions those chunks hold.
    pub damaged_regions: u64,
}

/// Options that refuse no chunk a daemon could have written: up to 64 MiB, the most
/// any configuration derives. A daemon may have been configured with larger chunks
/// than a default build, and recovery reads a frame at a time anyway.
fn reading_options() -> StorageOptions {
    let mut opts = StorageOptions::from_config(&Config::default());
    opts.max_chunk_bytes = 64 << 20;
    opts.max_chunks_per_segment = usize::MAX;
    opts
}

/// Reads the metadata log and every segment of the database at `root`, checking each
/// chunk's CRCs. Changes nothing. Needs no key: a chunk's CRCs cover what is stored.
///
/// # Errors
/// [`DatabaseError::Metadata`] for a log that does not replay;
/// [`DatabaseError::Storage`] for what stops a stream being assessed;
/// [`DatabaseError::Io`].
pub fn inspect(root: &Path) -> Result<Inspection> {
    let records = inspect_log(&root.join(METADATA_DIR))?.records;
    let mut found = Inspection {
        records,
        ..Inspection::default()
    };
    let streams = root.join("streams");
    if !streams.exists() {
        return Ok(found);
    }
    let entries = fs::read_dir(&streams).map_err(|e| DatabaseError::io("list", &streams, e))?;
    for entry in entries {
        let dir = entry
            .map_err(|e| DatabaseError::io("list", &streams, e))?
            .path();
        let report = recover_stream(&dir, &reading_options(), RecoveryMode::ReportOnly)?;
        found.verified_segments += report.verified_segments;
        found.damaged_regions += report.corruptions().count() as u64;
    }
    Ok(found)
}
