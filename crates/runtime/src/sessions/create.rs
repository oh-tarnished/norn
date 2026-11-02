//! Starting a session.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use norn_replay::{ReplayError, Session, SessionSpec};
use tokio::sync::mpsc;

use super::driver::Driver;
use super::shared::{Entry, Shared};
use super::{Replays, SessionInfo};

/// Requests queued for a session's driver. A caller past this waits.
const REQUEST_QUEUE: usize = 16;

impl Replays {
    /// A new session, paused at its start. `id` names it, or one is chosen. `scope`
    /// and `filter` are kept to show back. Must be called inside a tokio runtime.
    ///
    /// # Errors
    /// [`ReplayError::TooManySessions`], [`ReplayError::AlreadyExists`],
    /// [`ReplayError::ShuttingDown`].
    pub fn create(
        &self,
        id: Option<String>,
        spec: SessionSpec,
        labels: (String, String),
    ) -> Result<SessionInfo, ReplayError> {
        if self.closing.load(Ordering::Acquire) {
            return Err(ReplayError::ShuttingDown);
        }
        let now = self.clock.now();
        self.expire(now);
        let mut list = self.list();
        if list.len() >= self.limit {
            return Err(ReplayError::TooManySessions { limit: self.limit });
        }
        let taken = |wanted: &str| list.iter().any(|e| e.id == wanted);
        let id = match id {
            Some(id) if taken(&id) => return Err(ReplayError::AlreadyExists),
            Some(id) => id,
            None => loop {
                let mut next = self.next_id.lock().unwrap_or_else(PoisonError::into_inner);
                *next += 1;
                let chosen = format!("replay-{next}");
                if !taken(&chosen) {
                    break chosen;
                }
            },
        };
        let (start, end) = (spec.start, spec.end);
        let session = Session::new(spec, Arc::clone(&self.engine));
        let shared = Arc::new(Shared::new(session.view(), now));
        let (tx, requests) = mpsc::channel(REQUEST_QUEUE);
        let driver = Driver {
            session: Some(session),
            requests,
            shared: Arc::clone(&shared),
            clock: Arc::clone(&self.clock),
            token: self.token.clone(),
            subscribers: Vec::new(),
            outbox: VecDeque::new(),
            numbered: 0,
            stepping: None,
            stalled: false,
        };
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        while tasks.try_join_next().is_some() {}
        tasks.spawn(driver.drive());
        let entry = Arc::new(Entry {
            id,
            scope: labels.0,
            filter: labels.1,
            start,
            end,
            created: now,
            shared,
            requests: tx,
        });
        list.push_back(Arc::clone(&entry));
        Ok(entry.info(now, self.idle))
    }
}
