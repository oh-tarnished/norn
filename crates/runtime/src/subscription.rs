//! The receiving end of a subscription, live or replayed: one abstraction (SPEC §27).

use std::sync::{Arc, Mutex, PoisonError};

use norn_core::Timestamp;
use norn_replay::ReplayError;
use tokio::sync::mpsc;

/// Why a subscription ended.
///
/// Concurrency: plain owned data, `Clone`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum End {
    /// Its session was stopped, at this reading of the clock.
    Stopped(Timestamp),
    /// Reading history failed; the session paused before the failure.
    Failed(ReplayError),
    /// A live subscriber fell behind its bounded queue.
    TooSlow,
    /// The daemon is shutting down.
    ShuttingDown,
}

/// Where the sending side leaves the reason it let go.
pub(crate) type EndSlot = Arc<Mutex<Option<End>>>;

pub(crate) fn leave(slot: &EndSlot, end: End) {
    let mut held = slot.lock().unwrap_or_else(PoisonError::into_inner);
    held.get_or_insert(end);
}

/// Items of a subscription, in order and with no gap, then why it ended.
///
/// Dropping it is how a subscriber leaves. Concurrency: `Send`, one reader. Memory:
/// its queue is bounded by whoever made it; a reader that stops reading either slows
/// a replay's clock or, live, is ended with [`End::TooSlow`].
#[derive(Debug)]
pub struct Subscription<T> {
    pub(crate) rx: mpsc::Receiver<T>,
    pub(crate) end: EndSlot,
    pub(crate) _guard: Option<super::sessions::Attached>,
}

impl<T> Subscription<T> {
    /// The next item if one is queued, without waiting. `None` says nothing about
    /// whether the subscription has ended: [`next`](Self::next) does.
    pub fn try_next(&mut self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// The next item. Everything queued is delivered before the end is reported, so
    /// the last event seen is followed by nothing silently.
    ///
    /// # Errors
    /// Why the subscription is over. Cancel-safe.
    pub async fn next(&mut self) -> Result<T, End> {
        if let Some(item) = self.rx.recv().await {
            return Ok(item);
        }
        let mut held = self.end.lock().unwrap_or_else(PoisonError::into_inner);
        Err(held.take().unwrap_or(End::ShuttingDown))
    }
}
