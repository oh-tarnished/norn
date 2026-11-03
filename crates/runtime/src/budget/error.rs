//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Why a reservation was refused.

use std::fmt;
use std::time::Duration;

/// A refused [`Budget`](crate::Budget) reservation. Match on the variant:
/// `TooLarge` is permanent for that size, the others are transient.
///
/// Each variant names the budget, so a caller reporting it upward (as
/// `RESOURCE_EXHAUSTED`, or the write path's `REJECT` with
/// `MEMORY_EXHAUSTED`) can say which subsystem was full.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BudgetError {
    /// The request exceeds the whole budget, so it can never be granted.
    /// Split the work, or configure a larger budget.
    TooLarge {
        /// The budget's name, such as `ingest_bytes`.
        budget: &'static str,
        /// Bytes asked for.
        requested: u64,
        /// The budget's capacity.
        capacity: u64,
    },
    /// A non-waiting reservation found too little free. Retry after others
    /// release, or wait with [`Budget::reserve`](crate::Budget::reserve).
    Exhausted {
        /// The budget's name.
        budget: &'static str,
        /// Bytes asked for.
        requested: u64,
        /// Bytes free when it was refused.
        available: u64,
    },
    /// A waiting reservation reached its bound without enough being freed.
    TimedOut {
        /// The budget's name.
        budget: &'static str,
        /// Bytes asked for.
        requested: u64,
        /// Bytes free when it gave up.
        available: u64,
        /// How long it waited, on the budget's clock.
        waited: Duration,
    },
    /// The budget was closed (the runtime is shutting down), so it grants
    /// nothing further. Outstanding reservations remain valid until dropped.
    Closed {
        /// The budget's name.
        budget: &'static str,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge {
                budget,
                requested,
                capacity,
            } => write!(
                f,
                "{budget}: {requested} bytes exceeds the whole budget of {capacity}"
            ),
            Self::Exhausted {
                budget,
                requested,
                available,
            } => write!(f, "{budget}: {requested} bytes requested, {available} free"),
            Self::TimedOut {
                budget,
                requested,
                available,
                waited,
            } => write!(
                f,
                "{budget}: {requested} bytes requested, {available} free after waiting {waited:?}"
            ),
            Self::Closed { budget } => write!(f, "{budget}: closed for shutdown"),
        }
    }
}

impl std::error::Error for BudgetError {}
