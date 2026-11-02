//! What callers read of a session without asking its task.

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use norn_core::Timestamp;
use norn_replay::{ReplayState, SessionView};
use tokio::sync::mpsc;

use super::driver::Request;

/// One session as a caller sees it, at one moment.
///
/// Concurrency: plain owned data.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// The last part of its name.
    pub id: String,
    /// Whose signals, as given at creation.
    pub scope: String,
    /// Which of them, as given at creation.
    pub filter: String,
    /// The first instant replayed.
    pub start: Timestamp,
    /// The instant past the last one replayed.
    pub end: Timestamp,
    /// State, pace, counters, generation.
    pub view: SessionView,
    /// The clock's reading.
    pub current_time: Timestamp,
    /// When it was created, on the daemon's clock.
    pub created: Timestamp,
    /// When its controls last changed.
    pub updated: Timestamp,
    /// When it goes unless something touches it.
    pub expires: Timestamp,
}

#[derive(Debug)]
struct Published {
    view: SessionView,
    updated: Timestamp,
}

/// Written by the session's task, read by anyone.
#[derive(Debug)]
pub(crate) struct Shared {
    published: Mutex<Published>,
    touched: AtomicI64,
    attached: AtomicUsize,
}

impl Shared {
    pub(super) fn new(view: SessionView, now: Timestamp) -> Self {
        Self {
            published: Mutex::new(Published { view, updated: now }),
            touched: AtomicI64::new(now.as_nanos()),
            attached: AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Published> {
        self.published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn view(&self) -> SessionView {
        self.lock().view
    }

    pub(super) fn publish(&self, view: SessionView, now: Timestamp) {
        let mut published = self.lock();
        if published.view.generation != view.generation {
            published.updated = now;
        }
        published.view = view;
    }

    pub(super) fn touch(&self, now: Timestamp) {
        self.touched.store(now.as_nanos(), Ordering::Release);
    }

    fn expires(&self, idle: Duration) -> Timestamp {
        let nanos = i64::try_from(idle.as_nanos()).unwrap_or(i64::MAX);
        let touched = self.touched.load(Ordering::Acquire);
        Timestamp::from_nanos(touched.saturating_add(nanos))
    }

    pub(super) fn expired(&self, now: Timestamp, idle: Duration) -> bool {
        if self.attached.load(Ordering::Acquire) > 0 {
            self.touch(now);
            return false;
        }
        now >= self.expires(idle)
    }
}

/// Counts one subscriber for as long as it is held.
#[derive(Debug)]
pub struct Attached(Arc<Shared>);

impl Attached {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        shared.attached.fetch_add(1, Ordering::AcqRel);
        Self(shared)
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.0.attached.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
pub(super) struct Entry {
    pub(super) id: String,
    pub(super) scope: String,
    pub(super) filter: String,
    pub(super) start: Timestamp,
    pub(super) end: Timestamp,
    pub(super) created: Timestamp,
    pub(super) shared: Arc<Shared>,
    pub(super) requests: mpsc::Sender<Request>,
}

impl Entry {
    pub(super) fn info(&self, now: Timestamp, idle: Duration) -> SessionInfo {
        let published = self.shared.lock();
        let view = published.view;
        let running = view.state == ReplayState::Running;
        let reading = view.clock.now(now);
        SessionInfo {
            id: self.id.clone(),
            scope: self.scope.clone(),
            filter: self.filter.clone(),
            start: self.start,
            end: self.end,
            view,
            current_time: if running {
                reading.min(self.end)
            } else {
                reading
            },
            created: self.created,
            updated: published.updated,
            expires: self.shared.expires(idle),
        }
    }
}
