//! LATEST: the newest matching sample of one signal, walking back from the end.

use std::sync::Arc;

use norn_core::{Sample, Timestamp};
use norn_index::IndexError;

use crate::error::{QueryError, Result};
use crate::filter::Filter;
use crate::read::{Reader, Stop};
use crate::selection::Selection;
use crate::spec::Target;

/// The earliest instant the newest sample at or before `hi` can be at, from the
/// nearest chunk and the tail. `None` when nothing is at or before `hi`.
///
/// A chunk ending by `hi` holds a sample at exactly its end. One straddling `hi`
/// holds one somewhere from its start. Either way the newest sample of all, which
/// may be in another chunk under overlap, is no earlier.
fn lower_bound(reader: &Reader, target: &Target, hi: Timestamp) -> Result<Option<Timestamp>> {
    let engine = &reader.engine;
    let tail = engine
        .tail
        .newest_at_or_before(target.stream, target.signal, hi);
    let mut retries = engine.limits.race_retries;
    let stored = loop {
        match engine
            .index
            .snapshot()
            .nearest_at_or_before(target.signal, hi)
        {
            Ok(found) => break found,
            Err(IndexError::SegmentGone { stream, .. }) if retries > 0 => {
                retries -= 1;
                match engine.index.refresh(stream) {
                    Ok(_) | Err(IndexError::PersistFailed(_)) => {}
                    Err(e) => return Err(QueryError::Index(e)),
                }
            }
            Err(IndexError::SegmentGone { .. }) => return Err(QueryError::Raced),
            Err(e) => return Err(QueryError::Index(e)),
        }
    };
    let stored = stored.map(|l| {
        if l.max_ts() <= hi {
            l.max_ts()
        } else {
            l.min_ts()
        }
    });
    Ok(tail.max(stored))
}

/// The newest sample of `target` in `from..=to` passing `filter`.
///
/// One small read when the newest sample passes, however long the signal has been
/// silent: `nearest_at_or_before` finds its chunk from the per-signal summary.
/// With a filter it steps back chunk by chunk until one matches, which on a
/// filter nothing passes reads the signal's whole history, in bounded memory.
pub(crate) fn latest(
    reader: &mut Reader,
    index: u32,
    target: &Arc<Target>,
    (from, to): (Timestamp, Timestamp),
    filter: &Filter,
) -> Result<Option<Sample>> {
    let selection = Selection::new(vec![(index, Arc::clone(target))]);
    let limit = reader.engine.limits.slice_bytes;
    let mut hi = to;
    while hi >= from {
        let Some(low) = lower_bound(reader, target, hi)? else {
            return Ok(None);
        };
        let mut lo = low.max(from);
        let found = loop {
            let cap = if lo == hi { u64::MAX } else { limit };
            match reader.read(&selection, lo, hi, filter, cap) {
                Ok((rows, _charge)) => break rows.into_iter().next_back(),
                Err(Stop::Failed(e)) => return Err(e),
                Err(Stop::Narrow) if lo == hi => return Err(QueryError::TooLarge),
                Err(Stop::Memory(e)) if lo == hi => return Err(e.into()),
                Err(_) => {
                    let span = hi.as_nanos().abs_diff(lo.as_nanos()) / 2;
                    lo = Timestamp::from_nanos(
                        hi.as_nanos()
                            .saturating_sub(i64::try_from(span).unwrap_or(i64::MAX)),
                    );
                }
            }
        };
        if let Some(row) = found {
            return Ok(Some(row.sample));
        }
        let Some(before) = lo.as_nanos().checked_sub(1) else {
            return Ok(None);
        };
        hi = Timestamp::from_nanos(before);
    }
    Ok(None)
}
