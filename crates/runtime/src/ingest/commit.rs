//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The commit thread: journal first, then buffer; flush; group fsync; seal.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use norn_core::Timestamp;
use tokio::sync::oneshot;

use super::Context;
use super::buffer::{COLUMN_BYTES, Generation, payload};
use super::decision::{AckFailure, AckLevel};

use super::journal::Journal;
use super::shared::{Shared, Work};

/// How often an idle stream looks at its buffers' age.
const TICK: Duration = Duration::from_millis(100);
/// The most batches taken before they are flushed and acknowledged.
const MAX_DRAIN: usize = 256;
/// Waiters that end a group's wait at once: nothing is gained by holding more.
const MAX_GROUP: usize = 512;

type Ack = (AckLevel, oneshot::Sender<Result<AckLevel, AckFailure>>);

/// One stream's commit loop. Its fsync wait is a real-time bound, not the
/// injected clock: it paces a disk, and no logic depends on where it falls.
pub(crate) struct Committer {
    ctx: Arc<Context>,
    pub(super) shared: Arc<Shared>,
    journal: Journal,
    pub(super) store: Option<SyncSender<Generation>>,
    pub(super) unflushed: VecDeque<Ack>,
    pub(super) unsynced: VecDeque<Ack>,
    group_since: Option<Instant>,
}

impl Committer {
    pub(crate) fn new(
        ctx: Arc<Context>,
        shared: Arc<Shared>,
        journal: Journal,
        store: SyncSender<Generation>,
    ) -> Self {
        Self {
            ctx,
            shared,
            journal,
            store: Some(store),
            unflushed: VecDeque::new(),
            unsynced: VecDeque::new(),
            group_since: None,
        }
    }

    /// Runs until admission hangs up, then commits what is left.
    pub(crate) fn run(mut self, rx: &Receiver<Work>) {
        if let Err(e) = self.commit_loop(rx) {
            self.fail(&e, rx);
        }
    }

    fn commit_loop(&mut self, rx: &Receiver<Work>) -> Result<(), String> {
        loop {
            match rx.recv_timeout(self.wait()) {
                Ok(work) => {
                    self.take(work)?;
                    for _ in 0..MAX_DRAIN {
                        let Ok(work) = rx.try_recv() else { break };
                        self.take(work)?;
                    }
                    self.flush()?;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return self.finish(),
            }
            let elapsed = self
                .group_since
                .is_some_and(|t| t.elapsed() >= self.ctx.group_interval);
            if elapsed || self.unsynced.len() >= MAX_GROUP {
                self.sync()?;
            }
            let now = self.ctx.clock.now();
            let (age, empty, bytes) = {
                let state = self.shared.tail.read();
                let b = &state.buffers;
                (b.age(now), b.is_empty(), b.bytes())
            };
            let aged = age >= i64::try_from(self.ctx.max_chunk_age.as_nanos()).unwrap_or(i64::MAX);
            if !empty && (aged || bytes >= self.ctx.cap / 2) {
                self.seal()?;
            }
        }
    }

    fn wait(&self) -> Duration {
        self.group_since.map_or(TICK, |t| {
            self.ctx.group_interval.saturating_sub(t.elapsed())
        })
    }

    /// Journals a batch, then moves it to columns and shrinks its reservation.
    fn take(&mut self, mut work: Work) -> Result<(), String> {
        self.shared.queued.fetch_sub(1, Ordering::AcqRel);
        if !work.samples.is_empty() {
            self.journal
                .append(&work.samples)
                .map_err(|e| e.to_string())?;
            let mut columns = 0;
            let mut range: Option<(Timestamp, Timestamp)> = None;
            let mut state = self.shared.tail.write();
            for s in &work.samples {
                state.buffers.push(s).map_err(|e| e.to_string())?;
                columns += COLUMN_BYTES + payload(s);
                let t = s.timestamp;
                range = Some(range.map_or((t, t), |(a, b)| (a.min(t), b.max(t))));
            }
            let before = work.memory.bytes();
            work.memory.shrink_to(columns);
            let freed = before.saturating_sub(work.memory.bytes());
            self.shared.held.fetch_sub(freed, Ordering::AcqRel);
            state.buffers.adopt(work.memory, self.ctx.clock.now());
            drop(state);
            let lock = self.shared.stats.lock();
            let mut stats = lock.unwrap_or_else(PoisonError::into_inner);
            stats.buffer(work.samples.len() as u64, range);
            drop(stats);
            // After the buffers, so a subscriber's next query sees what it was told.
            let samples = std::mem::take(&mut work.samples);
            self.ctx.live.publish(self.shared.stream, samples);
        }
        if let Some(ack) = work.ack {
            self.unflushed.push_back((work.level, ack));
        }
        Ok(())
    }

    /// `PERSISTED`: the bytes are in the OS.
    fn flush(&mut self) -> Result<(), String> {
        self.journal.flush().map_err(|e| e.to_string())?;
        while let Some((level, ack)) = self.unflushed.pop_front() {
            if level >= AckLevel::Durable {
                self.group_since.get_or_insert_with(Instant::now);
                self.unsynced.push_back((level, ack));
            } else if self.unsynced.is_empty() {
                let _ = ack.send(Ok(level));
            } else {
                // Keeps second responses in stream order behind a waiting fsync.
                self.unsynced.push_back((level, ack));
            }
        }
        Ok(())
    }

    /// `DURABLE`: reported only after the fsync has returned.
    fn sync(&mut self) -> Result<(), String> {
        self.ctx.registry.commit().map_err(|e| e.to_string())?;
        if let Some(gate) = &self.ctx.gate {
            gate.before_sync().map_err(|e| e.to_string())?;
        }
        self.journal.sync().map_err(|e| e.to_string())?;
        if let Some(gate) = &self.ctx.gate {
            gate.after_sync();
        }
        self.group_since = None;
        while let Some((level, ack)) = self.unsynced.pop_front() {
            let _ = ack.send(Ok(level));
        }
        Ok(())
    }

    fn seal(&mut self) -> Result<(), String> {
        self.flush()?;
        if self.group_since.is_some() {
            self.sync()?;
        }
        let journal = self.journal.roll().map_err(|e| e.to_string())?;
        let generation = self.shared.tail.seal(journal);
        let store = self.store.as_ref().ok_or("store thread gone")?;
        store
            .send(generation)
            .map_err(|_| "the store thread stopped".to_owned())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.seal()?;
        self.store = None;
        Ok(())
    }
}
