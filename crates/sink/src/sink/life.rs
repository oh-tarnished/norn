//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! [`Sink::run`] and [`Sink::close`]: a sink's one life, only forward.

use super::Sink;
use crate::error::SinkError;
use crate::health::{Notice, Report};
use crate::pump;
use crate::shared::{Life, Shared};

/// Marks a run over however it ends, its future dropped included, so `close`
/// never waits for a run that is gone.
struct Running<'a>(&'a Shared);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.closing.send_replace(true);
        self.0.life.send_replace(Life::Stopped);
    }
}

impl Sink {
    /// Delivers until [`close`](Self::close) and its drain, or until this
    /// future is dropped: connects, provisions, appends, and when the daemon
    /// goes away waits and starts again. The daemon failing is never an error
    /// here; it is told through [`reach`](Self::reach) and
    /// [`Options::on_notice`](crate::Options::on_notice).
    ///
    /// # Errors
    ///
    /// [`SinkError::AlreadyRunning`] if another call is running, and
    /// [`SinkError::Closed`] once one has ended or `close` has been called.
    pub async fn run(&self) -> Result<Report, SinkError> {
        let mut claimed = Ok(());
        self.shared.life.send_if_modified(|life| match life {
            Life::Idle => {
                *life = Life::Running;
                true
            }
            Life::Running => {
                claimed = Err(SinkError::AlreadyRunning);
                false
            }
            Life::Stopped => {
                claimed = Err(SinkError::Closed);
                false
            }
        });
        claimed?;
        let _running = Running(&self.shared);
        Ok(pump::run(&self.shared).await)
    }

    /// Stops taking readings, gives what is held up to [`Options::drain`](crate::Options::drain) to be
    /// acknowledged, and returns once [`run`](Self::run) has stopped. Idempotent,
    /// and safe from any task.
    ///
    /// A `run` that has not started yet, as on a task just spawned, still drains
    /// what is held if it starts within that time. If none does, what is held is
    /// given up and told as [`Notice::NeverSent`], and a later `run` is
    /// [`SinkError::Closed`]; so is one started after closing a sink holding
    /// nothing, which has nothing to do.
    pub async fn close(&self) {
        self.shared.closing.send_replace(true);
        self.shared.wake.notify_one();
        let mut life = self.shared.life.subscribe();
        let holds = {
            let state = self.shared.state();
            !state.work.is_empty() || state.flow.held() > 0
        };
        if holds {
            let started = life.wait_for(|now| *now != Life::Idle);
            let _ = tokio::time::timeout(self.shared.options.drain, started).await;
        }
        let never_ran = self.shared.life.send_if_modified(|life| {
            let idle = *life == Life::Idle;
            if idle {
                *life = Life::Stopped;
            }
            idle
        });
        if never_ran {
            let readings = self.shared.state().flow.abandon();
            if readings > 0 {
                self.shared.tell(&Notice::NeverSent { readings });
            }
        }
        // The sender lives as long as `self`, so this only ends by the value.
        let _ = life.wait_for(|now| *now == Life::Stopped).await;
    }
}
