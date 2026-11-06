//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! [`Sink`]: what an adapter holds. Its two calls that take data never wait on
//! the daemon, so Norn being slow or absent slows nothing else.

use crate::counts::Counts;
use crate::error::SinkError;
use crate::flow::Flow;
use crate::health::{Health, Reach};
use crate::ids;
use crate::options::Options;
use crate::provision::Work;
use crate::shared::{Life, Shared, State};
use norn_ipc::Endpoint;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, watch};

mod accept;
mod life;

pub use accept::{Declared, Pushed};

/// How often [`Sink::settled`] looks at what is left.
const SETTLE_LOOK: Duration = Duration::from_millis(25);

/// One stream's delivery into one daemon.
///
/// Concurrency: `Send + Sync`, and [`Clone`] gives another handle on the same
/// sink, so [`declare`](Self::declare) and [`push`](Self::push) may come from
/// any number of tasks while one of them awaits [`run`](Self::run). Readings
/// pushed from one task keep their order; the order between tasks is theirs.
/// [`new`](Self::new) spawns nothing, and `run` starts no task of its own: all
/// it does happens on the task that awaits it. Dropping that future stops the
/// sink as [`close`](Self::close) would, without the drain.
#[derive(Clone)]
pub struct Sink {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Sink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sink")
            .field("endpoint", &self.shared.endpoint)
            .field("stream", &self.shared.stream)
            .finish_non_exhaustive()
    }
}

impl Sink {
    /// A sink of the stream `stream` (its ID) at `endpoint`. Touches nothing:
    /// the stream is created by [`run`](Self::run), if it does not exist.
    ///
    /// # Errors
    ///
    /// [`SinkError::InvalidId`] for a stream ID Norn would refuse.
    pub fn new(endpoint: Endpoint, stream: &str, options: Options) -> Result<Self, SinkError> {
        if !ids::is_valid(stream) {
            return Err(SinkError::InvalidId {
                field: "stream_id",
                id: stream.to_owned(),
            });
        }
        let flow = Flow::new(ids::stream_name(stream), options.ack, options.buffer_bytes);
        let state = State {
            flow,
            work: Work::default(),
            entities: HashMap::new(),
            signals: HashMap::new(),
            made: HashSet::new(),
            sequence: 1,
            dropped_at: None,
            published_limit: None,
        };
        let shared = Shared {
            endpoint,
            stream: stream.to_owned(),
            options,
            state: Mutex::new(state),
            wake: Notify::new(),
            closing: watch::Sender::new(false),
            life: watch::Sender::new(Life::Idle),
            reach: watch::Sender::new(Reach::default()),
        };
        let shared = Arc::new(shared);
        Ok(Self { shared })
    }

    /// Waits until nothing is held and nothing is left to create. Gives
    /// nothing up, and never ends while the daemon is away: the caller bounds
    /// the wait, and on timing out what is held stays held.
    pub async fn settled(&self) {
        loop {
            {
                let state = self.shared.state();
                if state.work.is_empty() && state.flow.held() == 0 {
                    return;
                }
            }
            tokio::time::sleep(SETTLE_LOOK).await;
        }
    }

    /// Whether Norn answers, as a `watch` that keeps only the newest.
    #[must_use]
    pub fn reach(&self) -> watch::Receiver<Reach> {
        self.shared.reach.subscribe()
    }

    /// How delivery stands now.
    #[must_use]
    pub fn health(&self) -> Health {
        let reach = self.shared.reach.borrow().clone();
        let state = self.shared.state();
        let counts = state.flow.counts();
        let window = self.shared.options.degraded_for;
        Health {
            reach,
            delivered: counts.acknowledged,
            held: state.flow.held() as u64,
            dropped: counts.dropped(),
            degraded: state.dropped_at.is_some_and(|t| t.elapsed() < window),
            unmade: state.flow.unmade(),
        }
    }

    /// What this sink has counted so far.
    #[must_use]
    pub fn counts(&self) -> Counts {
        self.shared.state().flow.counts()
    }
}
