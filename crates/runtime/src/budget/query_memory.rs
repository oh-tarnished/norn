//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The query engine's account: this budget, seen through `norn-query`'s trait.

use std::time::Duration;

use norn_query::{QueryMemory, Refused};

use super::{Budget, BudgetError};

/// How long a query waits for room: 200 tries, 5 ms apart. A plain count, so no
/// clock is read: nothing here depends on when it ends, only that it does.
const TRIES: u32 = 200;
const PAUSE: Duration = Duration::from_millis(5);

fn refused(e: BudgetError, requested: u64) -> Refused {
    match e {
        BudgetError::Closed { .. } => Refused::Closed,
        _ => Refused::Exhausted { requested },
    }
}

impl QueryMemory for Budget {
    /// Waits about a second for room, on the blocking thread a query runs on.
    fn acquire(&self, bytes: u64) -> Result<(), Refused> {
        for _ in 1..TRIES {
            match self.try_acquire(bytes) {
                Err(Refused::Exhausted { .. }) => std::thread::sleep(PAUSE),
                other => return other,
            }
        }
        self.try_acquire(bytes)
    }

    fn try_acquire(&self, bytes: u64) -> Result<(), Refused> {
        let mut held = self.try_reserve(bytes).map_err(|e| refused(e, bytes))?;
        // From here the query's `Charge` owns these bytes.
        held.bytes = 0;
        Ok(())
    }

    fn release(&self, bytes: u64) {
        Budget::release(self, bytes);
    }
}
