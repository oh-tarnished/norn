//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The write-ahead journal: raw batches in ordinary segments, so `PERSISTED` and
//! `DURABLE` come within milliseconds while chunks wait to grow.

use std::fmt;
use std::path::{Path, PathBuf};

use norn_core::{Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value, ValueKind};
use norn_storage::{Chunk, StorageOptions, StreamWriter};

use super::record;

/// The journal's directory name inside a stream directory.
pub const JOURNAL_DIR: &str = "journal";

/// Observes the journal's fsync: the seam that lets a test assert `DURABLE` is
/// never reported before the fsync has returned, or make one fail.
///
/// Called on the stream's commit thread. Blocking in either method blocks that
/// stream's commits, which is the point in a test.
pub trait SyncGate: Send + Sync + fmt::Debug {
    /// Runs before each fsync; an error is treated as a failed fsync.
    ///
    /// # Errors
    /// Whatever the test wants the fsync to fail with.
    fn before_sync(&self) -> std::io::Result<()> {
        Ok(())
    }

    /// Runs after each fsync has returned, before anything is acknowledged.
    fn after_sync(&self) {}
}

/// The journal of `stream_dir`.
pub(crate) fn journal_dir(stream_dir: &Path) -> PathBuf {
    stream_dir.join(JOURNAL_DIR)
}

/// Options for journal segments: batches go in uncompressed. They live for
/// seconds to minutes, and compressing them would spend the commit thread.
pub(crate) fn journal_options(opts: &StorageOptions) -> StorageOptions {
    let mut journal = *opts;
    journal.compression = norn_codec::compress::Compression::None;
    journal
}

/// One stream's journal writer.
///
/// Memory: the storage writer's bounds plus one batch's encoding. Failure: any
/// error poisons the writer beneath it; the commit thread then fails the stream.
#[derive(Debug)]
pub(crate) struct Journal {
    writer: StreamWriter,
    opts: StorageOptions,
    segments: Vec<PathBuf>,
}

impl Journal {
    pub(crate) fn open(
        dir: &Path,
        stream: StreamId,
        opts: &StorageOptions,
    ) -> Result<Self, norn_storage::StorageError> {
        let opts = journal_options(opts);
        let writer = StreamWriter::open(dir, stream, opts)?;
        Ok(Self {
            writer,
            opts,
            segments: Vec::new(),
        })
    }

    /// Appends one batch as one frame. Its timestamp is a constant, so a journal
    /// segment never rolls on sample time.
    pub(crate) fn append(&mut self, samples: &[Sample]) -> Result<(), JournalError> {
        let mut bytes = Vec::new();
        record::encode(samples, &mut bytes).map_err(JournalError::Encode)?;
        let mut chunk = Chunk::new(SignalId::new(0), ValueKind::Bytes)?;
        chunk.push(&Sample::new(
            Timestamp::UNIX_EPOCH,
            SignalId::new(0),
            Value::Bytes(bytes),
            Quality::GOOD,
            Sequence::ZERO,
        ))?;
        let appended = self.writer.append_encoded(&chunk.encode(&self.opts)?)?;
        if self.segments.last() != Some(&appended.segment) {
            self.segments.push(appended.segment);
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<(), norn_storage::StorageError> {
        self.writer.flush()
    }

    pub(crate) fn sync(&mut self) -> Result<(), norn_storage::StorageError> {
        self.writer.sync()
    }

    /// Seals the open segment and hands over every segment written since the
    /// last roll: they now belong to one generation.
    pub(crate) fn roll(&mut self) -> Result<Vec<PathBuf>, norn_storage::StorageError> {
        self.writer.roll()?;
        Ok(std::mem::take(&mut self.segments))
    }
}

/// Why a journal append failed.
#[derive(Debug)]
pub(crate) enum JournalError {
    Encode(norn_core::Error),
    Storage(norn_storage::StorageError),
}

impl From<norn_storage::StorageError> for JournalError {
    fn from(e: norn_storage::StorageError) -> Self {
        Self::Storage(e)
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "{e}"),
            Self::Storage(e) => write!(f, "{e}"),
        }
    }
}
