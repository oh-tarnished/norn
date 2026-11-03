//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovering one segment file.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use super::quarantine::{copy_region, move_file, truncate};
use super::scan::full_scan;
use super::{Action, RecoveryEvent, RecoveryMode, SegmentRecovery};
use crate::error::{Bound, Corruption, Result, StorageError};
use crate::format::HEADER_LEN;
use crate::format::directory;
use crate::format::frame::ChunkMeta;
use crate::fsio::Source;
use crate::options::StorageOptions;
use crate::segment::SegmentSummary;
use crate::segment::load::{load_directory, read_frame, read_header};

/// Verifies the segment at `path` and, in [`RecoveryMode::Repair`], makes
/// it whole again.
///
/// - **Unusable header** (torn, bad magic or CRC): the file is moved to
///   quarantine whole; a [`RecoveryEvent::SegmentQuarantined`] says why.
/// - **Sealed**: every listed chunk is read and its CRCs checked. A corrupt
///   chunk is reported and left in place; its reads keep failing with a
///   typed error, and its neighbours stay readable.
/// - **Unsealed** (crash before sealing) **or a damaged seal**: every frame is
///   verified by a full scan. Each damaged region is copied to quarantine;
///   a region at the end of the file (the torn tail) is then truncated; and
///   the segment is sealed over the complete chunks.
///
/// Report-only mode reports the same findings and changes nothing. Running
/// recovery twice is safe: the second run finds a clean, sealed segment.
///
/// Cost: reads the whole file once. Memory: one frame and the directory.
///
/// # Errors
/// [`StorageError::Io`]; [`StorageError::UnsupportedVersion`] for a file
/// from a newer Norn, which is left untouched; [`StorageError::TooLarge`]
/// for more frames than `max_chunks_per_segment`.
pub fn recover_segment(
    path: &Path,
    opts: &StorageOptions,
    mode: RecoveryMode,
) -> Result<SegmentRecovery> {
    let repair = mode == RecoveryMode::Repair;
    let mut src = Source::open(path)?;
    let header = match read_header(&mut src) {
        Ok(h) => h,
        Err(StorageError::Corrupt(corruption)) => {
            drop(src);
            let to = if repair { Some(move_file(path)?) } else { None };
            let events = vec![RecoveryEvent::SegmentQuarantined { corruption, to }];
            return Ok(SegmentRecovery {
                summary: None,
                events,
            });
        }
        Err(e) => return Err(e),
    };
    let mut events = Vec::new();
    let len = src.len();
    match load_directory(&mut src, opts) {
        Ok(Some(chunks)) => {
            let mut buf = Vec::new();
            for meta in &chunks {
                match read_frame(&mut src, meta, opts, &mut buf) {
                    Ok(()) => {}
                    Err(StorageError::Corrupt(corruption)) => {
                        let action = if repair {
                            Action::LeftInPlace
                        } else {
                            Action::Reported
                        };
                        events.push(RecoveryEvent::Corruption { corruption, action });
                    }
                    Err(e) => return Err(e),
                }
            }
            let summary = SegmentSummary::new(path.to_path_buf(), header, &chunks, len);
            return Ok(SegmentRecovery {
                summary: Some(summary),
                events,
            });
        }
        Ok(None) | Err(StorageError::Corrupt(_) | StorageError::TooLarge { .. }) => {}
        Err(e) => return Err(e),
    }
    let scan = full_scan(&mut src, HEADER_LEN as u64, len, opts)?;
    drop(src);
    if scan.chunks.len() > opts.max_chunks_per_segment {
        let (len, max) = (scan.chunks.len() as u64, opts.max_chunks_per_segment as u64);
        return Err(StorageError::TooLarge {
            bound: Bound::SegmentChunks,
            len,
            max,
        });
    }
    let mut end = len;
    for d in &scan.damage {
        let corruption = Corruption {
            path: path.to_path_buf(),
            offset: d.offset,
            len: d.len,
            kind: d.kind,
        };
        let action = if !repair {
            Action::Reported
        } else {
            let to = copy_region(path, d.offset, d.len)?;
            if d.offset + d.len == len {
                truncate(path, d.offset)?;
                end = d.offset;
                Action::QuarantinedAndTruncated { to }
            } else {
                Action::Quarantined { to }
            }
        };
        events.push(RecoveryEvent::Corruption { corruption, action });
    }
    if repair {
        end = seal_at(path, &scan.chunks, end)?;
        let chunks = scan.chunks.len();
        events.push(RecoveryEvent::Sealed {
            path: path.to_path_buf(),
            chunks,
        });
    }
    let summary = SegmentSummary::new(path.to_path_buf(), header, &scan.chunks, end);
    Ok(SegmentRecovery {
        summary: Some(summary),
        events,
    })
}

/// The segment at `path` from its header and directory alone, if `vouched` holds
/// for its length and both verify. `None` means read it properly.
pub(super) fn read_vouched(
    path: &Path,
    opts: &StorageOptions,
    vouched: impl Fn(u64) -> bool,
) -> Result<Option<SegmentRecovery>> {
    let mut src = Source::open(path)?;
    let len = src.len();
    if !vouched(len) {
        return Ok(None);
    }
    let Ok(header) = read_header(&mut src) else {
        return Ok(None);
    };
    let Ok(Some(chunks)) = load_directory(&mut src, opts) else {
        return Ok(None);
    };
    let summary = SegmentSummary::new(path.to_path_buf(), header, &chunks, len);
    Ok(Some(SegmentRecovery {
        summary: Some(summary),
        events: Vec::new(),
    }))
}

/// Appends a directory of `chunks` and the trailer at offset `at` (the
/// current end of file), fsyncs, and returns the new length.
fn seal_at(path: &Path, chunks: &[ChunkMeta], at: u64) -> Result<u64> {
    let mut bytes = Vec::new();
    directory::encode(chunks, at, &mut bytes);
    let mut f = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| StorageError::io("open", path, e))?;
    f.seek(SeekFrom::Start(at))
        .and_then(|_| f.write_all(&bytes))
        .and_then(|()| f.sync_all())
        .map_err(|e| StorageError::io("seal", path, e))?;
    Ok(at + bytes.len() as u64)
}
