//! The daemon's lifecycle state, observable by anyone holding a watch.

use std::fmt;

use tokio::sync::watch;

/// Where the runtime is in its lifecycle. It only ever moves forward:
/// `Starting` → `Serving` → `Draining` → `Stopped`, possibly skipping steps
/// (a runtime closed before it ran goes straight to `Stopped`).
///
/// `#[non_exhaustive]`: a new state (such as recovering storage) may be
/// added; match with a wildcard arm and treat unknown states as not serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Health {
    /// Constructed, not yet running. No work is accepted.
    Starting,
    /// Running: every task is spawned and work is accepted.
    Serving,
    /// Shutdown requested: tasks have been told to stop and are finishing
    /// in-flight work. No new work is accepted.
    Draining,
    /// Every task has stopped. Terminal.
    Stopped,
}

impl Health {
    /// Whether the runtime accepts new work in this state.
    #[must_use]
    pub fn is_serving(self) -> bool {
        self == Self::Serving
    }
}

impl fmt::Display for Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Starting => "starting",
            Self::Serving => "serving",
            Self::Draining => "draining",
            Self::Stopped => "stopped",
        })
    }
}

/// A read-only view of a runtime's [`Health`], updated as it changes.
///
/// Cheap to clone; each clone tracks independently which change it has
/// seen. Holds no reference to the runtime: once the runtime is dropped,
/// the watch reports [`Health::Stopped`]. Concurrency: `Send + Sync`; a
/// slow reader never blocks the runtime, it simply sees only the latest
/// state (intermediate states may be skipped, never reordered).
#[derive(Debug, Clone)]
pub struct HealthWatch {
    rx: watch::Receiver<Health>,
}

impl HealthWatch {
    pub(crate) fn new(rx: watch::Receiver<Health>) -> Self {
        Self { rx }
    }

    /// The current state.
    #[must_use]
    pub fn get(&self) -> Health {
        *self.rx.borrow()
    }

    /// Waits for a state this watch has not yet seen, and returns it.
    ///
    /// Returns [`Health::Stopped`] at once if the runtime has been dropped.
    /// Cancel-safe.
    pub async fn changed(&mut self) -> Health {
        match self.rx.changed().await {
            Ok(()) => *self.rx.borrow_and_update(),
            Err(_) => Health::Stopped,
        }
    }

    /// Waits until the state is at least `target` in lifecycle order (or
    /// the runtime is dropped) and returns the state reached. Cancel-safe.
    pub async fn reached(&mut self, target: Health) -> Health {
        match self.rx.wait_for(|h| *h >= target).await {
            Ok(h) => *h,
            Err(_) => Health::Stopped,
        }
    }
}
