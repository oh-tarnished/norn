//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One read: a time slice of some signals, from buffers and chunks, exactly once.

use std::ops::ControlFlow;
use std::sync::Arc;

use norn_codec::MAX_BLOCK_BYTES;
use norn_core::{Sample, StreamId, Timestamp, ValueKind};
use norn_index::{ChunkLocator, IndexError, PlanRequest, Snapshot};
use norn_storage::{Chunk, StorageError};

use crate::engine::Engine;
use crate::error::{ChunkDamage, QueryError};
use crate::files::{FileCache, FileError, replaced};
use crate::filter::Filter;
use crate::memory::{Charge, Refused};
use crate::rows::{Row, Rows};
use crate::selection::Selection;
use crate::tail::TailSource;

/// What a sample costs decoded: time, sequence, an inline value, a quality.
const DECODED_SAMPLE: u64 = 26;
/// What a `String` or `Vec<u8>` costs apart from its bytes.
const PAYLOAD_HEADER: u64 = 24;

/// Why a read did not finish.
#[derive(Debug)]
pub(crate) enum Stop {
    /// More than this read may hold: narrow the slice and read again.
    Narrow,
    /// The query budget refused it.
    Memory(Refused),
    /// A failure no narrowing helps.
    Failed(QueryError),
}

/// Why one attempt at a read did not finish.
enum Attempt {
    Stop(Stop),
    /// A segment of this stream vanished. Nothing was delivered, so refresh and
    /// read the slice again from scratch: that is the whole of the race handling.
    Raced(StreamId),
}

/// What reads share: open files and counters.
#[derive(Debug)]
pub(crate) struct Reader {
    pub(crate) engine: Arc<Engine>,
    pub(crate) files: FileCache,
    pub(crate) chunks_read: u64,
    pub(crate) races: u64,
}

impl Reader {
    /// Every matching sample of `selection` in `from..=to`, sorted by (time,
    /// sequence, target), each (signal, time, sequence) once, with the charge
    /// paying for them.
    ///
    /// **Why this is exactly-once.** The tail is read before the snapshot is
    /// taken, and a sample leaves the tail only after it entered the index
    /// ([`crate::TailSource`]). So it is in one or the other or both, and equal
    /// rows collapse here. Rows held past `cap` bytes stop the read with
    /// [`Stop::Narrow`].
    pub(crate) fn read(
        &mut self,
        selection: &Selection,
        from: Timestamp,
        to: Timestamp,
        filter: &Filter,
        cap: u64,
    ) -> Result<(Vec<Row>, Charge), Stop> {
        for _ in 0..=self.engine.limits.race_retries {
            match self.attempt(selection, from, to, filter, cap) {
                Ok(done) => return Ok(done),
                Err(Attempt::Stop(stop)) => return Err(stop),
                Err(Attempt::Raced(stream)) => {
                    self.races += 1;
                    self.refresh(stream)?;
                }
            }
        }
        Err(Stop::Failed(QueryError::Raced))
    }

    fn refresh(&self, stream: StreamId) -> Result<(), Stop> {
        match self.engine.index.refresh(stream) {
            Ok(_) | Err(IndexError::PersistFailed(_)) => Ok(()),
            Err(e) => Err(Stop::Failed(QueryError::Index(e))),
        }
    }

    fn attempt(
        &mut self,
        selection: &Selection,
        from: Timestamp,
        to: Timestamp,
        filter: &Filter,
        cap: u64,
    ) -> Result<(Vec<Row>, Charge), Attempt> {
        let memory = Arc::clone(&self.engine.memory);
        let mut rows = Rows::new(Charge::new(Arc::clone(&memory)), filter, cap, (from, to));
        for (index, target) in &selection.targets {
            let mut failed = None;
            let mut visit = |sample: Sample| match rows.offer(*index, sample) {
                Ok(()) => ControlFlow::Continue(()),
                Err(stop) => {
                    failed = Some(stop);
                    ControlFlow::Break(())
                }
            };
            let tail = &self.engine.tail;
            tail.scan(target.stream, target.signal, from, to, &mut visit);
            if let Some(stop) = failed {
                return Err(Attempt::Stop(stop));
            }
        }
        // Second, and only now: see `read`.
        rows.may_wait = true;
        let snapshot = self.engine.index.snapshot();
        self.files.next_read();
        let request = PlanRequest::new(from, to).signals(selection.filter.clone());
        for locator in snapshot.plan(request) {
            let locator = locator.map_err(planning)?;
            // `_decoded` pays for the chunk until its rows are copied out.
            let (chunk, _decoded) = self.chunk(&snapshot, &locator, &memory)?;
            if let Some(target) = selection.target_of(chunk.signal()) {
                rows.take(target, &chunk, &locator).map_err(Attempt::Stop)?;
            }
        }
        Ok(rows.finish())
    }

    fn chunk(
        &mut self,
        snapshot: &Snapshot,
        locator: &ChunkLocator,
        memory: &Arc<dyn crate::memory::QueryMemory>,
    ) -> Result<(Chunk, Charge), Attempt> {
        let mut decoded = Charge::new(Arc::clone(memory));
        decoded
            .grow(decoded_bytes(locator))
            .map_err(|e| Attempt::Stop(Stop::Memory(e)))?;
        self.chunks_read += 1;
        match self.files.read(snapshot, locator) {
            Ok(chunk) => Ok((chunk, decoded)),
            Err(FileError::Memory(e)) => Err(Attempt::Stop(Stop::Memory(e))),
            Err(FileError::Storage(e)) => Err(classify(e, locator, self.engine.tail.as_ref())),
        }
    }
}

fn planning(e: IndexError) -> Attempt {
    match e {
        IndexError::SegmentGone { stream, .. } => Attempt::Raced(stream),
        IndexError::PlanTooLarge { .. } => Attempt::Stop(Stop::Narrow),
        other => Attempt::Stop(Stop::Failed(QueryError::Index(other))),
    }
}

/// A vanished file is a race; damage is reported as damage.
fn classify(e: StorageError, l: &ChunkLocator, tail: &dyn TailSource) -> Attempt {
    let damage = |source| {
        tail.damaged(l.stream, l.segment, l.offset(), l.chunk.count);
        let damage = ChunkDamage {
            stream: l.stream,
            segment: l.segment,
            offset: l.offset(),
            signal: l.chunk.signal,
            min_ts: l.min_ts(),
            max_ts: l.max_ts(),
            source,
        };
        Attempt::Stop(Stop::Failed(QueryError::Corrupt(Box::new(damage))))
    };
    match e {
        StorageError::Io { source, .. } if replaced(&source) => Attempt::Raced(l.stream),
        e @ (StorageError::Corrupt(_) | StorageError::Codec { .. }) => damage(e),
        other => Attempt::Stop(Stop::Failed(QueryError::Storage(other))),
    }
}

/// The most a chunk can take decoded. A payload column can decode to a block of
/// `MAX_BLOCK_BYTES`, and only decoding tells how much less it is, so it is
/// charged in full: over-charged rather than unaccounted.
fn decoded_bytes(l: &ChunkLocator) -> u64 {
    let count = u64::from(l.chunk.count);
    let columns = count.saturating_mul(DECODED_SAMPLE);
    match l.chunk.kind {
        ValueKind::String | ValueKind::Bytes => columns
            .saturating_add(count.saturating_mul(PAYLOAD_HEADER))
            .saturating_add(MAX_BLOCK_BYTES as u64),
        _ => columns,
    }
}
