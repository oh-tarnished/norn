//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Visiting the chunks of one sealed segment that match a filter.

use norn_core::{StreamId, Timestamp};
use norn_storage::format::frame::ChunkMeta;

use super::SignalFilter;
use crate::detail::{read_head, read_page};
use crate::error::Result;
use crate::record::SegmentRecord;
use crate::shared::Shared;

/// Calls `f` for each chunk of `record` matching `filter` and overlapping
/// `from..=to`, in `(signal, min_ts, offset)` order.
///
/// Reads the head and only the pages that can hold a filtered signal, through
/// the cache. A segment whose signal range rules the filter out costs nothing.
///
/// # Errors
/// [`crate::IndexError::SegmentGone`], when sidecar and segment are unusable.
pub(crate) fn chunks_of(
    shared: &Shared,
    stream: StreamId,
    record: &SegmentRecord,
    filter: &SignalFilter,
    from: Timestamp,
    to: Timestamp,
    mut f: impl FnMut(&ChunkMeta),
) -> Result<()> {
    if record.chunks == 0 || !filter.may_hold(record.min_signal, record.max_signal) {
        return Ok(());
    }
    let head = read_head(shared, stream, record)?;
    let mut visit = |page| -> Result<()> {
        let chunks = read_page(shared, stream, record, &head, page)?;
        chunks
            .iter()
            .filter(|c| filter.matches(c.signal) && c.overlaps(from, to))
            .for_each(&mut f);
        Ok(())
    };
    let Some(signals) = filter.signals() else {
        for page in 0..head.pages() {
            visit(page)?;
        }
        return Ok(());
    };
    // Signals ascend and so do their pages, so `next` visits each page once.
    let mut next = 0;
    for signal in signals {
        let pages = head.pages_of(*signal);
        for page in pages.start.max(next)..pages.end {
            visit(page)?;
            next = page + 1;
        }
    }
    Ok(())
}
