//! Recovering a whole stream directory.

use std::fs;
use std::path::{Path, PathBuf};

use super::quarantine::remove;
use super::{RecoveryEvent, RecoveryMode, RecoveryReport, recover_segment};
use crate::error::{Result, StorageError};
use crate::layout::{SegmentEntry, superseded};
use crate::lock::StreamLock;
use crate::options::StorageOptions;
use crate::segment::{SEGMENT_EXT, SegmentName, SegmentSummary};

/// Recovers every segment in stream directory `dir`, then finishes any
/// compaction a crash interrupted:
///
/// 1. `*.nseg.tmp` files are compaction outputs that never committed (the
///    rename is the commit); their inputs are intact, so they are removed.
/// 2. Each segment goes through [`recover_segment`].
/// 3. A segment superseded by a committed compaction output (higher
///    generation, covering its sequence range) is removed, but only when
///    that output verified clean in this same run; otherwise both stay and
///    the corruption is reported.
///
/// Run it before opening a [`crate::StreamWriter`] on `dir`. In
/// [`RecoveryMode::ReportOnly`] the same events are reported and nothing
/// changes. Idempotent.
///
/// Repair holds the stream's [`StreamLock`] throughout, so it cannot run beside
/// a writer; report-only takes no lock and changes nothing.
///
/// Memory: one [`SegmentSummary`] per segment plus the events.
///
/// # Errors
/// [`StorageError::Locked`] when repairing a stream another writer holds;
/// [`StorageError::Io`]; whatever [`recover_segment`] returns for a
/// segment it cannot even assess (a newer format version).
pub fn recover_stream(
    dir: &Path,
    opts: &StorageOptions,
    mode: RecoveryMode,
) -> Result<RecoveryReport> {
    let repair = mode == RecoveryMode::Repair;
    let _lock = repair.then(|| StreamLock::acquire(dir)).transpose()?;
    let mut report = RecoveryReport::default();
    let (mut segments, uncommitted) = scan_dir(dir)?;
    for path in uncommitted {
        if repair {
            remove(&path)?;
        }
        report.events.push(RecoveryEvent::UncommittedRemoved {
            path,
            removed: repair,
        });
    }
    segments.sort();
    let mut recovered: Vec<(SegmentSummary, bool)> = Vec::new();
    for (_, path) in segments {
        let r = recover_segment(&path, opts, mode)?;
        let clean = !r.events.iter().any(RecoveryEvent::is_corruption);
        report.events.extend(r.events);
        if let Some(summary) = r.summary {
            recovered.push((summary, clean));
        }
    }
    let entries: Vec<SegmentEntry> = recovered
        .iter()
        .map(|(s, _)| SegmentEntry {
            path: s.path.clone(),
            header: s.header,
        })
        .collect();
    let mut gone: Vec<PathBuf> = Vec::new();
    for (old, by) in superseded(&entries) {
        let by_clean = recovered
            .iter()
            .any(|(s, clean)| s.path == by.path && *clean);
        if !by_clean {
            continue;
        }
        if repair {
            remove(&old.path)?;
        }
        gone.push(old.path.clone());
        let (path, by) = (old.path.clone(), by.path.clone());
        report.events.push(RecoveryEvent::SupersededRemoved {
            path,
            by,
            removed: repair,
        });
    }
    report.segments = recovered
        .into_iter()
        .map(|(s, _)| s)
        .filter(|s| !gone.contains(&s.path))
        .collect();
    Ok(report)
}

/// Segment files and uncommitted outputs in `dir`.
type Listing = (Vec<(SegmentName, PathBuf)>, Vec<PathBuf>);

fn scan_dir(dir: &Path) -> Result<Listing> {
    let tmp_suffix = format!(".{SEGMENT_EXT}.tmp");
    let (mut segments, mut tmps) = (Vec::new(), Vec::new());
    let entries = fs::read_dir(dir).map_err(|e| StorageError::io("list", dir, e))?;
    for entry in entries {
        let path = entry.map_err(|e| StorageError::io("list", dir, e))?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.ends_with(&tmp_suffix) {
            tmps.push(path);
        } else if let Some(n) = SegmentName::parse(&path) {
            segments.push((n, path));
        }
    }
    Ok((segments, tmps))
}
