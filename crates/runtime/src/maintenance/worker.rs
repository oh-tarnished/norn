//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The worker's loop: client jobs in order, and passes on the schedule.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::Maintenance;
use super::months;
use super::ops::{Failure, Operation, Outcome, Request};

impl Maintenance {
    /// Runs jobs and scheduled passes until `token` is cancelled, then ends every
    /// queued job as cancelled and returns. The job in progress stops as
    /// [`Operation::cancel`] says.
    pub async fn run(self: Arc<Self>, token: CancellationToken) {
        let taken = self
            .rx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(mut rx) = taken else { return };
        let c = self.config;
        let compaction = c.compaction_interval;
        let retention = c.retention_interval;
        let (mut compact_in, mut retain_in) = (compaction, retention);
        loop {
            let wait = compact_in.min(retain_in);
            let next = if c.background {
                wait
            } else {
                Duration::from_secs(3600)
            };
            tokio::select! {
                () = token.cancelled() => break,
                op = rx.recv() => match op {
                    Some(op) => self.run_job(op, &token).await,
                    None => break,
                },
                () = self.clock.sleep(next) => if c.background {
                    compact_in = compact_in.saturating_sub(wait);
                    retain_in = retain_in.saturating_sub(wait);
                    let (compact, retain) = (compact_in.is_zero(), retain_in.is_zero());
                    if compact {
                        compact_in = compaction;
                    }
                    if retain {
                        retain_in = retention;
                    }
                    self.run_pass(compact, retain, &token).await;
                },
            }
        }
        rx.close();
        while let Ok(op) = rx.try_recv() {
            op.finish(self.clock.now(), Err(Failure::Cancelled));
        }
    }

    async fn run_job(self: &Arc<Self>, op: Arc<Operation>, token: &CancellationToken) {
        let worker = Arc::clone(self);
        let cancel = op.cancel.clone();
        let stop = token.clone();
        // Shutdown stops the job the way a client's cancel would.
        let relay = stop.child_token();
        let watch = {
            let (relay, cancel) = (relay.clone(), cancel.clone());
            tokio::spawn(async move {
                relay.cancelled().await;
                cancel.cancel();
            })
        };
        let job = Arc::clone(&op);
        let done = tokio::task::spawn_blocking(move || worker.execute(&job, &cancel)).await;
        watch.abort();
        let result = done.unwrap_or_else(|_| Err(Failure::Failed("the job panicked".to_owned())));
        op.finish(self.clock.now(), result);
    }

    fn execute(&self, op: &Operation, cancel: &CancellationToken) -> Result<Outcome, Failure> {
        let stream = op.stream;
        match op.request {
            Request::Compact { interval } => self
                .compact_stream(stream, interval, Some(op), cancel, usize::MAX)
                .map(Outcome::Compacted),
            Request::Verify => self
                .verify_stream(stream, op, cancel)
                .map(Outcome::Verified),
            Request::Retention { dry_run } => {
                self.apply_retention(stream, dry_run).map(Outcome::Retained)
            }
        }
    }

    async fn run_pass(self: &Arc<Self>, compact: bool, retain: bool, token: &CancellationToken) {
        let worker = Arc::clone(self);
        let cancel = token.clone();
        let done = tokio::task::spawn_blocking(move || worker.pass(compact, retain, &cancel));
        if done.await.is_err() {
            tracing::error!("a maintenance pass panicked");
        }
    }

    fn pass(&self, compact: bool, retain: bool, cancel: &CancellationToken) {
        let ids = self.registry.view().stream_ids();
        let minute = months::minute_of_day(self.clock.now());
        let compact = compact && self.config.in_window(minute);
        let max = self.config.max_runs_per_pass;
        for id in ids {
            if cancel.is_cancelled() {
                return;
            }
            if retain {
                let _ = self.apply_retention(id, false);
            }
            if compact {
                let _ = self.compact_stream(id, None, None, cancel, max);
            }
        }
    }
}
