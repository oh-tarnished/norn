//! Injected time: the runtime never reads a clock it was not handed.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norn_core::Timestamp;
use tokio::sync::watch;

/// A future that completes once a [`Clock`]'s time has advanced far enough.
pub type Sleep<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// A source of time, injected so that every timeout, bound and timestamp the
/// runtime produces can be driven by a test (or a replay) instead of the wall.
///
/// Implementations must be `Send + Sync`: one clock is shared, through an
/// `Arc`, by the runtime and every budget it hands out. `sleep` must be
/// cancel-safe: dropping the returned future simply stops waiting.
pub trait Clock: Send + Sync + fmt::Debug {
    /// The current time.
    fn now(&self) -> Timestamp;

    /// Completes once at least `duration` has passed on this clock.
    fn sleep(&self, duration: Duration) -> Sleep<'_>;
}

/// The operating system's wall clock, with tokio's timer for `sleep`.
///
/// `sleep` needs a tokio runtime with the time driver enabled; outside one it
/// panics inside tokio, which is why the daemon always runs on one.
/// Concurrency: stateless, shareable freely.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_nanos())
                .map(|n| -n)
                .unwrap_or(i64::MIN),
        };
        Timestamp::from_nanos(nanos)
    }

    fn sleep(&self, duration: Duration) -> Sleep<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A clock that moves only when told to, for deterministic tests.
///
/// Clones share one time. [`advance`](FakeClock::advance) wakes every
/// sleeper whose deadline has been reached; nothing else ever does, so a
/// test controls exactly when each timeout fires. Concurrency: `Send +
/// Sync`; advancing from one task while others sleep is the intended use.
#[derive(Clone)]
pub struct FakeClock {
    now: Arc<watch::Sender<i64>>,
}

impl fmt::Debug for FakeClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeClock")
            .field("now", &self.now())
            .finish()
    }
}

impl FakeClock {
    /// A clock reading `start`.
    #[must_use]
    pub fn new(start: Timestamp) -> Self {
        let (now, _) = watch::channel(start.as_nanos());
        Self { now: Arc::new(now) }
    }

    /// Moves the clock forward by `by`, saturating at [`Timestamp::MAX`],
    /// and wakes every sleeper whose deadline is now reached.
    pub fn advance(&self, by: Duration) {
        let nanos = i64::try_from(by.as_nanos()).unwrap_or(i64::MAX);
        self.now.send_modify(|t| *t = t.saturating_add(nanos));
    }

    /// How many [`sleep`](Clock::sleep) futures are waiting right now, so a
    /// test can advance only once the code under test has started waiting.
    #[must_use]
    pub fn sleepers(&self) -> usize {
        self.now.receiver_count()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_nanos(*self.now.borrow())
    }

    fn sleep(&self, duration: Duration) -> Sleep<'_> {
        let nanos = i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX);
        let deadline = self.now.borrow().saturating_add(nanos);
        let mut rx = self.now.subscribe();
        Box::pin(async move {
            while *rx.borrow_and_update() < deadline {
                // The sender lives as long as `self`, which this future
                // borrows, so `changed` cannot fail while we wait.
                if rx.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}
