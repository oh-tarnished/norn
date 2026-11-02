//! Which segments are live: `live_segments`' rule, without its O(n²).

use std::collections::BTreeMap;

use crate::record::SegmentRecord;

/// Keeps the records no other record supersedes.
///
/// [`SegmentRecord::supersedes`] is the rule, the one
/// `norn_storage::live_segments` applies pair by pair; a property test holds the
/// two equal. Comparing every pair costs 1.2 billion comparisons at 35,040
/// segments, on every startup and update. This sweeps by `first_seq` instead,
/// tracking the highest `last_seq` seen per generation: `r` is superseded when a
/// higher generation has already reached `r.last_seq`. Generations number a
/// handful, so it is O(n log n), and the result keeps its input order.
pub(crate) fn retain_live(records: &mut Vec<SegmentRecord>) {
    let mut order: Vec<&SegmentRecord> = records.iter().collect();
    order.sort_by_key(|r| r.first_seq);
    let mut reach: BTreeMap<u16, u64> = BTreeMap::new();
    let mut dead = Vec::new();
    let mut rest = order.as_slice();
    while let Some(first) = rest.first().map(|r| r.first_seq) {
        // Every record sharing this first_seq goes in before any is judged,
        // since supersession allows equal bounds.
        let split = rest.partition_point(|r| r.first_seq == first);
        let (same, later) = rest.split_at(split);
        for r in same {
            let e = reach.entry(r.generation).or_insert(r.last_seq);
            *e = (*e).max(r.last_seq);
        }
        for r in same {
            let above = r.generation.checked_add(1).map(|g| reach.range(g..));
            if above.is_some_and(|mut a| a.any(|(_, last)| *last >= r.last_seq)) {
                dead.push(r.name());
            }
        }
        rest = later;
    }
    if !dead.is_empty() {
        records.retain(|r| !dead.contains(&r.name()));
    }
}
