//! Making a stream's index agree with its directory: at startup and on refresh.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use norn_core::StreamId;
use norn_storage::SegmentName;

use crate::build::{Scanned, scan_all};
use crate::detail::DETAIL_EXT;
use crate::error::{IndexError, Result};
use crate::record::SegmentRecord;
use crate::report::StreamReport;
use crate::shared::{Shared, detail_name};
use crate::signals::{self, SignalMap};
use crate::signals_file::{self, SIGNALS_FILE, fingerprint};
use crate::stats::add;
use crate::summary::{self, SUMMARY_FILE};
use crate::view::StreamView;

/// A segment file: its name, length and path, from one `read_dir` and `stat`.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    pub(crate) name: SegmentName,
    pub(crate) len: u64,
    pub(crate) path: PathBuf,
}

/// Every segment file in `dir`; empty when `dir` does not exist. A file removed
/// mid-listing is simply not listed. Reads no segment.
///
/// # Errors
/// [`IndexError::Io`] when the directory cannot be read.
pub(crate) fn list_files(dir: &Path) -> Result<Vec<Listed>> {
    let io = |source| IndexError::Io {
        op: "list",
        path: dir.to_path_buf(),
        source,
    };
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io)?;
        let path = entry.path();
        let Some(name) = SegmentName::parse(&path) else {
            continue;
        };
        if let Ok(meta) = entry.metadata() {
            out.push(Listed {
                name,
                len: meta.len(),
                path,
            });
        }
    }
    out.sort_by_key(|l| l.name);
    Ok(out)
}

/// Builds the view of `stream` at startup. Trusts a summary record whose name and
/// length match a file; reads the directory of everything else.
///
/// # Errors
/// [`IndexError::Io`], [`IndexError::Storage`] (recovery has not run), and
/// [`IndexError::WrongStream`]. A bad summary is not among them.
pub(crate) fn open_stream(
    shared: &Shared,
    stream: StreamId,
    dir: &Path,
) -> Result<(StreamView, StreamReport)> {
    let index_dir = shared.index_dir(stream);
    fs::create_dir_all(&index_dir).map_err(|source| IndexError::Io {
        op: "create directory",
        path: index_dir.clone(),
        source,
    })?;
    let budget = shared.options.memory_budget_bytes;
    let (known, state) = summary::load(&index_dir.join(SUMMARY_FILE), stream, budget);
    let known_count = known.len();
    let mut by_name: HashMap<_, _> = known.into_iter().map(|r| (r.name(), r)).collect();
    let listed = list_files(dir)?;
    let mut records = Vec::with_capacity(listed.len());
    let mut unknown = Vec::new();
    for file in listed {
        match by_name.remove(&file.name) {
            Some(r) if r.file_len == file.len => records.push(r),
            _ => unknown.push(file.path),
        }
    }
    let reused = records.len();
    let mut tails = Vec::new();
    for scanned in scan_all(shared, stream, &unknown)? {
        match scanned {
            Scanned::Sealed(r) => records.push(r),
            Scanned::Unsealed(t) => tails.push(Arc::new(t)),
        }
    }
    let tail_count = tails.len();
    let view = StreamView::new(stream, records, tails);
    let (view, signals_saved) = with_signals(shared, view)?;
    let live = view.segments.records.len() + tail_count;
    let report = StreamReport {
        stream,
        summary: state,
        reused,
        read: unknown.len(),
        dropped: known_count.saturating_sub(reused) + (reused + unknown.len()).saturating_sub(live),
        tails: tail_count,
        orphans_removed: remove_orphans(shared, stream, &view),
        persisted: signals_saved,
    };
    Ok((view, report))
}

/// `view` with its per-signal summary, from `signals.nsig` when that file is of
/// exactly these segments, or rebuilt and saved when not. The bool says whether
/// the file on disk now matches. Tails were not in the file, so they are added.
fn with_signals(shared: &Shared, view: StreamView) -> Result<(StreamView, bool)> {
    let stream = view.stream;
    let path = shared.index_dir(stream).join(SIGNALS_FILE);
    let print = fingerprint(&view.segments.records);
    let budget = shared.options.memory_budget_bytes;
    let usable = signals_file::load(&path, stream, print, budget).filter(|m| m.all_live(&view));
    if let Some(mut map) = usable {
        for tail in view.tails.iter() {
            map = signals::absorb_tail(&map, tail);
        }
        return Ok((view.with_signals(Arc::new(map)), true));
    }
    let map = signals::rebuild(shared, &view)?;
    let saved = signals_file::save(&path, stream, print, &map).is_ok();
    Ok((view.with_signals(map), saved))
}

/// Removes temp files and sidecars of segments that are not live: what a crash
/// between a compaction and its cleanup leaves. Best effort, since an orphan
/// costs only its bytes. Returns how many went.
pub(crate) fn remove_orphans(shared: &Shared, stream: StreamId, view: &StreamView) -> usize {
    let dir = shared.index_dir(stream);
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    let live: HashSet<String> = view
        .segments
        .records
        .iter()
        .map(|r| detail_name(r.name()))
        .collect();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let tmp = name.ends_with(".tmp");
        let stale_detail = name.ends_with(DETAIL_EXT) && !live.contains(name);
        if (tmp || stale_detail) && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Writes the summary of `records`, sorted by name so that identical sets give
/// identical bytes, and `signals` beside it.
///
/// # Errors
/// [`IndexError::PersistFailed`].
pub(crate) fn persist(
    shared: &Shared,
    stream: StreamId,
    records: &[SegmentRecord],
    signals: &SignalMap,
) -> Result<()> {
    let mut sorted = records.to_vec();
    sorted.sort_by_key(SegmentRecord::name);
    let dir = shared.index_dir(stream);
    let path = dir.join(SUMMARY_FILE);
    fs::create_dir_all(&dir)
        .map_err(|e| norn_storage::StorageError::Io {
            op: "create directory",
            path: dir.clone(),
            source: e,
        })
        .and_then(|()| summary::save(&path, stream, &sorted))
        .and_then(|()| {
            let path = dir.join(SIGNALS_FILE);
            signals_file::save(&path, stream, fingerprint(records), signals)
        })
        .map_err(IndexError::PersistFailed)?;
    add(&shared.counters.summary_writes, 1);
    Ok(())
}
