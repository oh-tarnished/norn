//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Charging a query's memory to a budget this crate cannot name.

use std::fmt;
use std::sync::Arc;

/// Why memory was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// Too little is free, and waiting did not help.
    Exhausted {
        /// Bytes asked for.
        requested: u64,
    },
    /// The budget has closed: the daemon is shutting down.
    Closed,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted { requested } => {
                write!(f, "the query budget cannot spare {requested} bytes")
            }
            Self::Closed => f.write_str("the query budget has closed"),
        }
    }
}

impl std::error::Error for Refused {}

/// The account query memory is charged to. A trait because this crate may not
/// import `norn-runtime`, whose `Budget` implements it.
///
/// Concurrency: shared by every query at once. `acquire` may block for a bound of
/// its own choosing; it is only called from where blocking is allowed.
pub trait QueryMemory: Send + Sync + fmt::Debug {
    /// Takes `bytes`, or refuses.
    ///
    /// # Errors
    /// [`Refused`]; nothing was taken.
    fn acquire(&self, bytes: u64) -> Result<(), Refused>;

    /// Takes `bytes` only if that needs no waiting. What a caller uses while
    /// holding a lock another subsystem needs. By default, refuses.
    ///
    /// # Errors
    /// [`Refused`]; nothing was taken.
    fn try_acquire(&self, bytes: u64) -> Result<(), Refused> {
        Err(Refused::Exhausted { requested: bytes })
    }

    /// Returns `bytes` taken earlier.
    fn release(&self, bytes: u64);
}

/// Bytes held against a [`QueryMemory`]; dropping it returns them.
///
/// Concurrency: `Send`; it travels with what it pays for.
#[derive(Debug)]
#[must_use = "dropping a charge releases its bytes at once"]
pub struct Charge {
    memory: Arc<dyn QueryMemory>,
    bytes: u64,
}

impl Charge {
    /// Nothing held yet, against `memory`.
    pub fn new(memory: Arc<dyn QueryMemory>) -> Self {
        Self { memory, bytes: 0 }
    }

    /// The bytes held.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Takes `bytes` more.
    ///
    /// # Errors
    /// [`Refused`]; what was held stays held.
    pub fn grow(&mut self, bytes: u64) -> Result<(), Refused> {
        if bytes > 0 {
            self.memory.acquire(bytes)?;
            self.bytes = self.bytes.saturating_add(bytes);
        }
        Ok(())
    }

    /// Takes `bytes` more without waiting.
    ///
    /// # Errors
    /// [`Refused`]; what was held stays held.
    pub fn try_grow(&mut self, bytes: u64) -> Result<(), Refused> {
        if bytes > 0 {
            self.memory.try_acquire(bytes)?;
            self.bytes = self.bytes.saturating_add(bytes);
        }
        Ok(())
    }

    /// Returns all but `keep` bytes. A `keep` above what is held changes nothing.
    pub fn shrink_to(&mut self, keep: u64) {
        if keep < self.bytes {
            self.memory.release(self.bytes - keep);
            self.bytes = keep;
        }
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        if self.bytes > 0 {
            self.memory.release(self.bytes);
        }
    }
}
