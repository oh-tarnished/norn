//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup's cut of a stream: an exact point in its order of admission.
//!
//! A cut travels the write path like a batch. The commit thread takes it in turn, so
//! everything admitted before it is in the generation it seals, and everything after
//! is in a later one. That generation is stored and fsynced as any is, then the store
//! thread seals the open segment and lists what is live, all before it looks at the
//! next generation. So the sealed segments in the answer hold exactly the samples
//! admitted before the cut, and need no journal: every sample in the journal at that
//! moment is either in them or from after it. Sealed segments never change.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use norn_core::StreamId;
use norn_storage::{live_segments, stream_dir};
use tokio_util::sync::CancellationToken;

use super::shared::Work;
use super::{AckLevel, Ingest, QUEUE_BATCHES};

/// How a store thread answers a cut: the live segments, or why it cannot say.
pub(crate) type CutReply = mpsc::Sender<Result<Vec<PathBuf>, String>>;

/// How often a job waiting on a cut, or for room in a full queue, looks at its
/// token. A cut waits its turn behind what is queued, as a batch would.
const POLL: Duration = Duration::from_millis(20);

/// Sent by the store thread, after sealing. A job that gave up is no one's concern.
pub(crate) fn answer(reply: &CutReply, dir: &Path) {
    let live = live_segments(dir)
        .map(|all| all.into_iter().map(|s| s.path).collect())
        .map_err(|e| e.to_string());
    let _ = reply.send(live);
}

/// A cut that has been made, or is on its way through the write path.
///
/// Concurrency: `Send`, owned by the job waiting for it.
#[derive(Debug)]
pub enum PendingCut {
    /// The stream has no writer, so what is on disk is the cut.
    Ready(Vec<PathBuf>),
    /// In the stream's queue.
    Queued(mpsc::Receiver<Result<Vec<PathBuf>, String>>),
}

impl PendingCut {
    /// The stream's sealed, live segments at the cut. Blocks on the store thread.
    ///
    /// # Errors
    /// A message for people, when the stream's storage failed or `cancel` fired.
    pub fn wait(self, cancel: &CancellationToken) -> Result<Vec<PathBuf>, String> {
        let rx = match self {
            Self::Ready(paths) => return Ok(paths),
            Self::Queued(rx) => rx,
        };
        loop {
            match rx.recv_timeout(POLL) {
                Ok(answer) => return answer,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("the stream's write path stopped before its cut".to_owned());
                }
                Err(RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    return Err("cancelled".to_owned());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl Ingest {
    /// Cuts `stream`: see this module. Returns once the cut has its place in the
    /// order of admission, which is what makes cuts of many streams close together,
    /// and may wait a moment for room in a full queue. Seals the open segment, so each
    /// backup of a busy stream leaves one more small segment for compaction.
    ///
    /// Run it with no compaction or retention in progress on the stream: the
    /// maintenance worker does, by being one job at a time.
    ///
    /// # Errors
    /// A message for people, when the stream's storage failed, the daemon is shutting
    /// down, or `cancel` fired.
    pub fn begin_cut(
        &self,
        stream: StreamId,
        cancel: &CancellationToken,
    ) -> Result<PendingCut, String> {
        let dir = stream_dir(&self.ctx.root, stream);
        let streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(handle) = streams.get(&stream).cloned() else {
            // Held across the listing: with no handle, no writer has an open segment,
            // and none can start until this lock is released.
            let listed = if dir.exists() {
                live_segments(&dir).map_err(|e| e.to_string())?
            } else {
                Vec::new()
            };
            drop(streams);
            let paths = listed.into_iter().map(|s| s.path).collect();
            return Ok(PendingCut::Ready(paths));
        };
        drop(streams);
        let (reply, rx) = mpsc::channel();
        let shared = &handle.shared;
        loop {
            if shared.failed.load(Ordering::Acquire) {
                return Err("this stream's storage failed; see the daemon's log".to_owned());
            }
            let gate = handle.lock();
            // As admission: `queued` is checked under this lock, and only this lock sends.
            if shared.queued.load(Ordering::Acquire) < QUEUE_BATCHES {
                let memory = self.ctx.budget.try_reserve(0).map_err(|e| e.to_string())?;
                let work = Work {
                    samples: Vec::new(),
                    memory,
                    level: AckLevel::Buffered,
                    ack: None,
                    cut: Some(reply),
                };
                shared.queued.fetch_add(1, Ordering::AcqRel);
                let tx = gate.tx.as_ref().ok_or("the daemon is shutting down")?;
                tx.try_send(work)
                    .map_err(|_| "the stream's write path stopped")?;
                return Ok(PendingCut::Queued(rx));
            }
            drop(gate);
            if cancel.is_cancelled() {
                return Err("cancelled".to_owned());
            }
            std::thread::sleep(POLL);
        }
    }
}
