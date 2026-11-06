//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the adapter's side of a [`Sink`](crate::Sink) and its run share.

use crate::catalog::Signal;
use crate::flow::Flow;
use crate::health::{Notice, Reach};
use crate::options::Options;
use crate::provision::Work;
use norn_ipc::Endpoint;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use tokio::sync::{Notify, watch};

/// What both sides change, behind one lock held only for a moment: neither
/// ever waits on the daemon while holding it.
#[derive(Debug)]
pub(crate) struct State {
    pub(crate) flow: Flow,
    pub(crate) work: Work,
    /// What this run has declared, by full name, as last declared: what is
    /// declared again unchanged costs nothing.
    pub(crate) entities: HashMap<String, crate::catalog::Entity>,
    pub(crate) signals: HashMap<String, Signal>,
    /// Signals Norn has made, or given up, this run: a later declaration of
    /// one, an update, never holds its readings back.
    pub(crate) made: HashSet<String>,
    /// The next sequence for a reading that brings none.
    pub(crate) sequence: i64,
    /// When a reading was last given up, for [`crate::Health::degraded`].
    pub(crate) dropped_at: Option<Instant>,
    /// The unary message limit the daemon publishes, if it does
    /// (`pump/limits.rs`).
    pub(crate) published_limit: Option<usize>,
}

/// Where a run is in its life. Only forward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Life {
    /// Constructed; `run` has not started.
    Idle,
    /// `run` is in its loop.
    Running,
    /// `run` has returned, or `close` came first: nothing runs again.
    Stopped,
}

/// The whole of a sink, behind its `Arc`.
pub(crate) struct Shared {
    pub(crate) endpoint: Endpoint,
    /// The stream's ID; its name is the flow's.
    pub(crate) stream: String,
    pub(crate) options: Options,
    pub(crate) state: Mutex<State>,
    /// Readings or work arrived: the run should look.
    pub(crate) wake: Notify,
    /// `close` was called. A `watch` so that a run started after it sees it.
    pub(crate) closing: watch::Sender<bool>,
    pub(crate) life: watch::Sender<Life>,
    pub(crate) reach: watch::Sender<Reach>,
}

impl Shared {
    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Tells `notice` to the adapter's callback, if it gave one. Never called
    /// under the state lock.
    pub(crate) fn tell(&self, notice: &Notice) {
        if let Some(f) = &self.options.on_notice {
            f(notice);
        }
    }

    /// Whether `close` has been called.
    pub(crate) fn is_closing(&self) -> bool {
        *self.closing.borrow()
    }

    /// Resolves once `close` has been called.
    pub(crate) async fn closed(&self) {
        let mut closing = self.closing.subscribe();
        // The sender lives as long as `self`, so this only ends by the value.
        let _ = closing.wait_for(|c| *c).await;
    }

    /// Norn answers or does not; only a change is told.
    pub(crate) fn set_reach(&self, up: bool, detail: String) {
        let next = Reach { up, detail };
        let changed = self.reach.send_if_modified(|now| {
            // A new reason is news too: the first refusal is rarely the last.
            let changed = *now != next;
            if changed {
                *now = next.clone();
            }
            changed
        });
        if changed {
            self.tell(&Notice::Reach(next));
        }
    }
}
