//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An immutable view of the whole index, and the nearest-chunk lookup.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use norn_core::{SignalId, StreamId};

use crate::locator::ChunkLocator;
use crate::plan::{Plan, PlanRequest};
use crate::record::SegmentRecord;
use crate::shared::Shared;
use crate::signals::SignalSpan;
use crate::view::StreamView;

/// What the index said at one instant. Immutable: it stays exactly as it was,
/// whatever ingest and compaction do afterwards.
///
/// Holding one pins its segment lists and tails in memory, uncounted by the
/// budget, so a long-lived holder should take a new one now and then. The files
/// it names can still disappear; see [`crate::TemporalIndex`].
///
/// Concurrency: `Send + Sync`. Lookups take `&self`, and any number may run at
/// once; they share only the page cache, one short mutex per access.
#[derive(Debug)]
pub struct Snapshot {
    pub(crate) epoch: u64,
    pub(crate) streams: BTreeMap<StreamId, Arc<StreamView>>,
    pub(crate) shared: Arc<Shared>,
}

impl Snapshot {
    /// Counts the updates installed before this snapshot. Larger means newer.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The streams known to this snapshot, in id order.
    pub fn streams(&self) -> impl Iterator<Item = StreamId> + '_ {
        self.streams.keys().copied()
    }

    /// `stream`'s sealed, live segments, sorted by `(min_ts, first_seq,
    /// generation)`. Empty for an unknown stream. Reads nothing.
    #[must_use]
    pub fn segments(&self, stream: StreamId) -> &[SegmentRecord] {
        self.streams
            .get(&stream)
            .map_or(&[], |v| v.segments.records.as_slice())
    }

    /// Chunks of `stream` in unsealed tails.
    #[must_use]
    pub fn tail_chunks(&self, stream: StreamId) -> usize {
        self.streams
            .get(&stream)
            .map_or(0, |v| v.tails.iter().map(|t| t.len()).sum())
    }

    /// The file holding `locator`'s chunk. Allocates; touches no disk.
    #[must_use]
    pub fn path_of(&self, locator: &ChunkLocator) -> PathBuf {
        self.shared.segment_path(locator.stream, locator.segment)
    }

    /// Streams every chunk matching `request`, in time order, with bounded
    /// memory. The one lookup behind signal → chunks, time range → chunks and
    /// stream → chunks. Does no work until iterated.
    #[must_use]
    pub fn plan(self: &Arc<Self>, request: PlanRequest) -> Plan {
        Plan::new(Arc::clone(self), request)
    }

    /// Every signal of `stream` with an indexed chunk, by id. Touches no disk.
    pub fn signals(&self, stream: StreamId) -> impl Iterator<Item = (SignalId, SignalSpan)> + '_ {
        let view = self.streams.get(&stream);
        view.into_iter()
            .flat_map(|v| v.signals.entries.iter().copied())
    }

    /// What the index knows of `signal`, over every stream, or `None` when no
    /// chunk of it is indexed. Touches no disk. O(streams × log signals).
    ///
    /// Samples still in the write path's buffers are not here. This is the bounded
    /// summary that keeps a lookup for a silent signal from walking its history.
    #[must_use]
    pub fn last_seen(&self, signal: SignalId) -> Option<SignalSpan> {
        let spans = self.streams.values().filter_map(|v| v.signals.get(signal));
        spans.reduce(|a, b| SignalSpan {
            first: a.first.min(b.first),
            ..if b.last > a.last { b } else { a }
        })
    }
}
