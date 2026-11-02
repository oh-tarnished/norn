//! Choosing what to compact and what retention removes: pure, over index records.

use norn_core::Timestamp;
use norn_index::SegmentRecord;
use norn_metadata::Retention;
use norn_storage::SegmentName;

use super::months::months_before;

/// How large one compaction's output may grow.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunLimits {
    /// Input bytes in one run, `max_segment_bytes`: compaction only shrinks, so
    /// the output fits a segment.
    pub(crate) max_bytes: u64,
    /// Sample time one run spans, `max_segment_span_nanos`.
    pub(crate) max_span_nanos: i64,
}

/// The next run of `records` to compact: neighbours in sequence order, none
/// skipped, within `limits`, all overlapping `interval` if one is given.
///
/// A run is two or more segments, or one segment fresh from ingest, whose chunks
/// merging and `ZstdSmallest` have not seen. So every compaction either lowers
/// the segment count or raises a segment's generation from zero, and asking again
/// after each one ends.
pub(crate) fn next_run(
    records: &[SegmentRecord],
    limits: RunLimits,
    interval: Option<(Timestamp, Timestamp)>,
    skip: &dyn Fn(SegmentName) -> bool,
) -> Vec<SegmentRecord> {
    let mut sorted = records.to_vec();
    sorted.sort_by_key(|r| r.first_seq);
    let wanted = |r: &SegmentRecord| {
        r.chunks > 0 && !skip(r.name()) && interval.is_none_or(|(a, b)| r.overlaps(a, b))
    };
    let mut run: Vec<SegmentRecord> = Vec::new();
    for r in sorted {
        if wanted(&r) && fits(&run, &r, limits) {
            run.push(r);
            continue;
        }
        if worthwhile(&run) {
            return run;
        }
        run.clear();
        if wanted(&r) {
            run.push(r);
        }
    }
    if worthwhile(&run) { run } else { Vec::new() }
}

fn worthwhile(run: &[SegmentRecord]) -> bool {
    run.len() > 1 || run.first().is_some_and(|r| r.generation == 0)
}

fn fits(run: &[SegmentRecord], next: &SegmentRecord, limits: RunLimits) -> bool {
    let bytes: u64 = run.iter().map(|r| r.file_len).sum();
    let from = run.iter().map(|r| r.min_ts).min().unwrap_or(next.min_ts);
    let to = run.iter().map(|r| r.max_ts).max().unwrap_or(next.max_ts);
    let span = to
        .max(next.max_ts)
        .as_nanos()
        .saturating_sub(from.min(next.min_ts).as_nanos());
    run.is_empty() || (bytes + next.file_len <= limits.max_bytes && span <= limits.max_span_nanos)
}

/// What `policy` removes from sealed `records`, oldest first, and the cutoff if it
/// has one. `newest` is the newest sample the stream holds, buffered or stored:
/// age is measured from it, so a stream gone quiet does not empty itself.
///
/// Whole segments only, each entirely past the limit. The segment with the
/// highest sequence number always stays, which also keeps sequence numbers from
/// being reused and a stale clean marker from matching a new file.
pub(crate) fn retention(
    records: &[SegmentRecord],
    policy: Retention,
    newest: Option<Timestamp>,
) -> (Vec<SegmentRecord>, Option<Timestamp>) {
    let last = records.iter().map(|r| r.last_seq).max();
    let mut candidates: Vec<SegmentRecord> = records
        .iter()
        .filter(|r| r.chunks > 0 && Some(r.last_seq) != last)
        .copied()
        .collect();
    candidates.sort_by_key(|r| (r.max_ts, r.first_seq));
    let cutoff = match (policy, newest) {
        (Retention::MaxAge(age), Some(t)) => {
            let nanos = i64::try_from(age.as_nanos()).unwrap_or(i64::MAX);
            Some(Timestamp::from_nanos(t.as_nanos().saturating_sub(nanos)))
        }
        (Retention::MaxMonths(n), Some(t)) => Some(months_before(t, n)),
        _ => None,
    };
    if let Some(cutoff) = cutoff {
        candidates.retain(|r| r.max_ts < cutoff);
        return (candidates, Some(cutoff));
    }
    let Retention::MaxBytes(limit) = policy else {
        return (Vec::new(), None);
    };
    let mut total: u64 = records.iter().map(|r| r.file_len).sum();
    candidates.retain(|r| {
        let over = total > limit;
        if over {
            total = total.saturating_sub(r.file_len);
        }
        over
    });
    (candidates, None)
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
