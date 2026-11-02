//! Memory budget accounting: reserve before allocating, release by drop.

mod error;
mod query_memory;
mod reservation;
mod set;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

use crate::clock::Clock;

pub use error::BudgetError;
pub use reservation::Reservation;
pub use set::{Budgets, Subsystem};

/// A byte budget shared by every user of one subsystem (SPEC §16).
///
/// A budget does not allocate; it accounts. Code reserves the bytes it is
/// about to hold, gets a [`Reservation`], and the bytes return to the budget
/// when the reservation is dropped. Reserving past the capacity is refused
/// ([`try_reserve`](Budget::try_reserve)) or waits for a release up to a
/// caller-given bound ([`reserve`](Budget::reserve)): that refusal or delay
/// is the backpressure SPEC §18 asks for, never an allocation failure.
///
/// Concurrency: `Clone + Send + Sync`; clones share one account. Accounting
/// is a lock-free compare-and-swap. Waiters are woken on every release and
/// retry, so there is no FIFO fairness: under a steady stream of small
/// reservations a large one may wait its whole bound. Performance: one
/// atomic operation per reserve and release when uncontended.
#[derive(Clone)]
pub struct Budget {
    inner: Arc<Inner>,
}

struct Inner {
    name: &'static str,
    capacity: u64,
    used: AtomicU64,
    closed: AtomicBool,
    released: Notify,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for Budget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Budget")
            .field("name", &self.inner.name)
            .field("capacity", &self.inner.capacity)
            .field("used", &self.used())
            .finish()
    }
}

impl Budget {
    /// A budget of `capacity` bytes named `name`, timing waits on `clock`.
    #[must_use]
    pub fn new(name: &'static str, capacity: u64, clock: Arc<dyn Clock>) -> Self {
        let inner = Inner {
            name,
            capacity,
            used: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            released: Notify::new(),
            clock,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// The budget's name, such as `ingest_bytes`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.inner.name
    }

    /// The most bytes it can have reserved at once.
    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.inner.capacity
    }

    /// Bytes reserved right now. A snapshot: it may change immediately.
    #[must_use]
    pub fn used(&self) -> u64 {
        self.inner.used.load(Ordering::Acquire)
    }

    /// Bytes free right now. A snapshot, like [`used`](Budget::used).
    #[must_use]
    pub fn available(&self) -> u64 {
        self.inner.capacity.saturating_sub(self.used())
    }

    /// Reserves `bytes` now, or refuses without waiting.
    ///
    /// # Errors
    ///
    /// [`BudgetError::TooLarge`] if `bytes` exceeds the capacity,
    /// [`BudgetError::Exhausted`] if too little is free,
    /// [`BudgetError::Closed`] after [`close`](Budget::close).
    pub fn try_reserve(&self, bytes: u64) -> Result<Reservation, BudgetError> {
        let budget = self.inner.name;
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(BudgetError::Closed { budget });
        }
        if bytes > self.inner.capacity {
            return Err(BudgetError::TooLarge {
                budget,
                requested: bytes,
                capacity: self.inner.capacity,
            });
        }
        let capacity = self.inner.capacity;
        let used = &self.inner.used;
        let mut current = used.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes).filter(|n| *n <= capacity) else {
                return Err(BudgetError::Exhausted {
                    budget,
                    requested: bytes,
                    available: capacity.saturating_sub(current),
                });
            };
            match used.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    return Ok(Reservation {
                        budget: self.clone(),
                        bytes,
                    });
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Reserves `bytes`, waiting up to `bound` (on the budget's clock) for
    /// other reservations to be released. Cancel-safe: dropping the future
    /// reserves nothing.
    ///
    /// # Errors
    ///
    /// As [`try_reserve`](Budget::try_reserve), except that a shortage
    /// becomes [`BudgetError::TimedOut`] once `bound` has passed.
    pub async fn reserve(&self, bytes: u64, bound: Duration) -> Result<Reservation, BudgetError> {
        let mut sleep = self.inner.clock.sleep(bound);
        loop {
            let released = self.inner.released.notified();
            tokio::pin!(released);
            // Registered before trying, so a release between the attempt and
            // the wait is not missed.
            released.as_mut().enable();
            match self.try_reserve(bytes) {
                Err(BudgetError::Exhausted { available, .. }) => {
                    tokio::select! {
                        () = &mut released => {}
                        () = &mut sleep => {
                            return Err(BudgetError::TimedOut {
                                budget: self.inner.name,
                                requested: bytes,
                                available,
                                waited: bound,
                            });
                        }
                    }
                }
                other => return other,
            }
        }
    }

    /// Refuses every further reservation and wakes every waiter, which
    /// fails with [`BudgetError::Closed`]. Idempotent. Reservations already
    /// granted stay valid and still return their bytes when dropped.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.inner.released.notify_waiters();
    }

    /// Returns `bytes` to the account and wakes waiters; only ever called
    /// by a [`Reservation`] for bytes it holds.
    pub(super) fn release(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        // Never underflows: only a reservation's own bytes are returned,
        // and they were added when it was granted.
        self.inner.used.fetch_sub(bytes, Ordering::AcqRel);
        self.inner.released.notify_waiters();
    }
}
