//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The store thread: encode a generation in parallel, write it in order, fsync,
//! and only then let go of its journal segments.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use norn_storage::{Chunk, EncodedChunk, StorageError, StorageOptions, StreamWriter, sync_dir};

use super::Context;
use super::buffer::Generation;
use super::indexing::Publisher;
use super::shared::Shared;

/// Chunks each encoding thread takes per round, so the frames waiting to be
/// written stay a small fraction of the generation.
const CHUNKS_PER_THREAD: usize = 256;
/// The most threads one stream encodes on.
const MAX_ENCODERS: usize = 4;

/// How often a stream whose index fell behind retries it.
const INDEX_RETRY: Duration = Duration::from_secs(1);

/// Encodes `chunks` on up to `workers` threads, keeping their order. Pure CPU;
/// the threads end before it returns.
fn encode(
    chunks: &[Chunk],
    opts: &StorageOptions,
    workers: usize,
) -> Result<Vec<EncodedChunk>, StorageError> {
    let size = chunks.len().div_ceil(workers.max(1)).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = chunks
            .chunks(size)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .map(|c| c.encode(opts))
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .collect();
        let mut out = Vec::with_capacity(chunks.len());
        for handle in handles {
            let part = handle
                .join()
                .map_err(|_| StorageError::InvalidChunk("an encoder panicked"))?;
            out.extend(part?);
        }
        Ok(out)
    })
}

/// Writes `generation` durably, then deletes its journal segments. The order is
/// the crash guarantee: a sample is in the journal until it is fsynced in a
/// segment, and recovery tolerates finding it in both.
pub(crate) fn store(
    writer: &mut StreamWriter,
    generation: Generation,
    opts: &StorageOptions,
    journal_dir: &Path,
    shared: &Shared,
    publisher: &mut Publisher<'_>,
) -> Result<u64, StorageError> {
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_ENCODERS);
    let (mut samples, mut bytes) = (0, 0);
    let frozen = &generation.frozen;
    for round in frozen.chunks.chunks(CHUNKS_PER_THREAD * workers) {
        for (encoded, chunk) in encode(round, opts, workers)?.iter().zip(round) {
            publisher.appended(writer.append_encoded(encoded)?);
            samples += chunk.len() as u64;
            bytes += encoded.len() as u64;
        }
    }
    writer.sync()?;
    publisher.synced(writer);
    for path in &generation.journal {
        std::fs::remove_file(path).map_err(|source| StorageError::Io {
            op: "remove",
            path: path.clone(),
            source,
        })?;
    }
    if !generation.journal.is_empty() {
        sync_dir(journal_dir)?;
    }
    let mut totals = shared.stats.lock().unwrap_or_else(PoisonError::into_inner);
    totals.stored(samples, bytes);
    let released = frozen.memory.as_ref().map_or(0, |m| m.bytes());
    Ok(released)
}

/// The store thread's body: until the commit thread hangs up. Failure marks the
/// stream failed; the data stays safe in the journal for recovery.
pub(crate) fn run(
    ctx: &Context,
    shared: &Arc<Shared>,
    dir: &Path,
    journal: &Path,
    rx: Receiver<Generation>,
) {
    let fail = |e: &StorageError| {
        tracing::error!(stream = shared.stream.get(), error = %e, "store failed");
        shared.failed.store(true, Ordering::Release);
    };
    let opts = ctx.opts;
    let mut writer = match StreamWriter::open(dir, shared.stream, opts) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut publisher = Publisher::new(ctx.index.get().map(Arc::as_ref), shared);
    // The newest generation stored and not yet known to be in the index.
    let mut unretired = None;
    let retire = |id: &mut Option<u64>| {
        if !shared.index_behind.load(Ordering::Acquire)
            && let Some(id) = id.take()
        {
            shared.tail.retire(id);
        }
    };
    loop {
        match rx.recv_timeout(INDEX_RETRY) {
            Ok(generation) => {
                let id = generation.frozen.id;
                match store(
                    &mut writer,
                    generation,
                    &opts,
                    journal,
                    shared,
                    &mut publisher,
                ) {
                    Ok(released) => {
                        shared.held.fetch_sub(released, Ordering::AcqRel);
                        unretired = Some(id);
                        retire(&mut unretired);
                    }
                    Err(e) => return fail(&e),
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                publisher.retry();
                retire(&mut unretired);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    match writer.close() {
        Ok(Some(summary)) => publisher.sealed(&summary),
        Ok(None) => {}
        Err(e) => fail(&e),
    }
}
