//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The newest sample of one signal in a range, for whoever asks signal by signal.

use std::sync::Arc;

use norn_core::{Sample, Timestamp};

use crate::engine::{Engine, QueryStats};
use crate::error::Result;
use crate::files::FileCache;
use crate::filter::Filter;
use crate::latest::latest;
use crate::read::Reader;
use crate::spec::Target;

/// LATEST for callers who need the whole sample, not an aggregate: `norn-state`.
///
/// Stored chunks and buffered samples are both seen, each (signal, time, sequence)
/// once, ties going to the highest sequence.
///
/// Lifecycle: from [`Engine::newest`]; dropping it releases everything.
/// Concurrency: `Send`, one caller at a time; [`at`](Newest::at) blocks on the disk.
/// Memory: one read's rows at a time, and up to `open_files` segment buffers, all
/// charged to the engine's memory. Performance: one or two chunk reads a signal,
/// or more when `to` lies inside a long gap in its history, since the index then
/// walks back across the gap.
#[derive(Debug)]
pub struct Newest {
    reader: Reader,
    filter: Filter,
}

impl Newest {
    pub(crate) fn new(engine: &Arc<Engine>, files: FileCache) -> Self {
        Self {
            reader: Reader {
                engine: Arc::clone(engine),
                files,
                chunks_read: 0,
                races: 0,
            },
            filter: Filter::default(),
        }
    }

    /// The newest sample of `target` in `from..=to`, or `None`.
    ///
    /// # Errors
    /// [`crate::QueryError::Corrupt`] for a damaged chunk, which is never reported
    /// as "no sample"; [`crate::QueryError::Memory`], [`crate::QueryError::Raced`].
    pub fn at(
        &mut self,
        target: &Arc<Target>,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Option<Sample>> {
        latest(&mut self.reader, 0, target, (from, to), &self.filter)
    }

    /// What these lookups have done so far.
    #[must_use]
    pub fn stats(&self) -> QueryStats {
        QueryStats {
            chunks_read: self.reader.chunks_read,
            bytes_read: self.reader.files.bytes_read(),
            races: self.reader.races,
        }
    }
}
