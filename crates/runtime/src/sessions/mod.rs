//! Replay sessions: created, driven on the injected clock, expired.

mod create;
mod delivery;
mod driver;
mod handle;
mod shared;

pub use shared::{Attached, SessionInfo};

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use norn_core::{Config, Timestamp};
use norn_query::Engine;
use norn_replay::{Batch, Control, ReplayError, ReplayState};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use self::driver::Request;
use self::shared::{Entry, Shared};
use crate::clock::Clock;
use crate::subscription::Subscription;

/// Every replay session of a daemon.
///
/// Lifecycle: [`Replays::new`] spawns nothing. Each session is a task, spawned by
/// [`create`](Replays::create) and owned here; [`run`](Replays::run) expires idle
/// sessions, and when cancelled stops them all and returns once every task has
/// ended. A task never outlives `run`, or this value if `run` never ran. Sessions
/// are not stored: a restart ends them. Time comes only from the injected clock.
///
/// Memory: at most `max_replay_sessions`, each reading through an engine charged to
/// the replay budget. Concurrency: `Send + Sync`, shared; a session's controls are
/// serialised by its task. Failure: typed, [`ReplayError`].
#[derive(Debug)]
pub struct Replays {
    engine: Arc<Engine>,
    clock: Arc<dyn Clock>,
    idle: Duration,
    limit: usize,
    sessions: Mutex<VecDeque<Arc<Entry>>>,
    tasks: Mutex<JoinSet<()>>,
    token: CancellationToken,
    closing: AtomicBool,
    next_id: Mutex<u64>,
}

impl Replays {
    /// Sessions reading through `engine`, with `config`'s limits.
    #[must_use]
    pub fn new(engine: Arc<Engine>, config: &Config, clock: Arc<dyn Clock>) -> Self {
        Self {
            engine,
            clock,
            idle: config.limits().replay_session_idle_timeout,
            limit: config.limits().max_replay_sessions,
            sessions: Mutex::new(VecDeque::new()),
            tasks: Mutex::new(JoinSet::new()),
            token: CancellationToken::new(),
            closing: AtomicBool::new(false),
            next_id: Mutex::new(0),
        }
    }

    fn list(&self) -> MutexGuard<'_, VecDeque<Arc<Entry>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn entry(&self, id: &str) -> Result<Arc<Entry>, ReplayError> {
        let entry = self.list().iter().find(|e| e.id == id).cloned();
        let entry = entry.ok_or(ReplayError::NotFound)?;
        entry.shared.touch(self.clock.now());
        Ok(entry)
    }

    /// A session as it is now. Asks no task, so a busy one answers at once.
    ///
    /// # Errors
    /// [`ReplayError::NotFound`].
    pub fn get(&self, id: &str) -> Result<SessionInfo, ReplayError> {
        let entry = self.entry(id)?;
        Ok(entry.info(self.clock.now(), self.idle))
    }

    /// Every session, oldest first. Touches none of them.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionInfo> {
        let now = self.clock.now();
        let list = self.list();
        list.iter().map(|e| e.info(now, self.idle)).collect()
    }

    async fn ask<T>(
        &self,
        entry: &Entry,
        make: impl FnOnce(driver::Reply<T>) -> Request,
    ) -> Result<T, ReplayError> {
        let (reply, answer) = oneshot::channel();
        let gone = |_| ReplayError::ShuttingDown;
        entry.requests.send(make(reply)).await.map_err(gone)?;
        answer.await.map_err(|_| ReplayError::ShuttingDown)?
    }

    /// Applies `control` if `generation` is unset or current, and returns the session
    /// after it. A step answers once its events have reached every subscriber.
    ///
    /// # Errors
    /// [`ReplayError::Conflict`], [`ReplayError::EtagMismatch`],
    /// [`ReplayError::OutsideInterval`], [`ReplayError::NotFound`],
    /// [`ReplayError::Query`] when history cannot be read.
    pub async fn control(
        &self,
        id: &str,
        generation: Option<u64>,
        control: Control,
    ) -> Result<SessionInfo, ReplayError> {
        let entry = self.entry(id)?;
        self.ask(&entry, |reply| Request::Control {
            control,
            generation,
            reply,
        })
        .await?;
        Ok(entry.info(self.clock.now(), self.idle))
    }

    /// Follows a session from its next event on.
    ///
    /// # Errors
    /// [`ReplayError::NotFound`]; [`ReplayError::Conflict`] when it has stopped.
    pub async fn subscribe(&self, id: &str) -> Result<Subscription<Arc<Batch>>, ReplayError> {
        let entry = self.entry(id)?;
        if entry.shared.view().state == ReplayState::Stopped {
            let (control, state) = ("subscribe to", "stopped");
            return Err(ReplayError::Conflict { control, state });
        }
        self.ask(&entry, |reply| Request::Attach { reply }).await
    }

    /// Stops a session if it is not stopped, and forgets it.
    ///
    /// # Errors
    /// [`ReplayError::NotFound`], [`ReplayError::EtagMismatch`].
    pub async fn delete(&self, id: &str, generation: Option<u64>) -> Result<(), ReplayError> {
        self.control(id, generation, Control::Stop).await?;
        self.list().retain(|e| e.id != id);
        Ok(())
    }

    /// Forgets sessions idle past the timeout. One with a subscriber is not idle.
    fn expire(&self, now: Timestamp) {
        self.list().retain(|e| !e.shared.expired(now, self.idle));
    }

    /// Expires sessions until `token` is cancelled, then ends every session and
    /// returns once all their tasks have.
    pub async fn run(&self, token: CancellationToken) {
        let tick = (self.idle / 4).clamp(Duration::from_millis(10), Duration::from_secs(30));
        loop {
            tokio::select! {
                () = token.cancelled() => break,
                () = self.clock.sleep(tick) => self.expire(self.clock.now()),
            }
        }
        self.closing.store(true, Ordering::Release);
        self.token.cancel();
        self.list().clear();
        let mut tasks = {
            let mut held = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *held)
        };
        while tasks.join_next().await.is_some() {}
    }
}
