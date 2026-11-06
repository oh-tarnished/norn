//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the server's targets run under: the daemon's own limits and budget type.

use std::sync::{Arc, OnceLock};

use norn_core::Config;
use norn_runtime::{Budget, SystemClock};
use norn_server::fuzzing::Limits;
use tokio::runtime::{Builder, Runtime};

/// The decode budget's size: eight times the default `ipc_bytes`, because the meter
/// caps a reservation at half the budget, and that cap must not hide its cost.
const BUDGET_BYTES: u64 = 256 << 20;

/// One runtime for the process; building it per input would be most of the cost.
///
/// # Panics
/// When the runtime cannot start: nothing can be tested without it.
pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        let mut builder = Builder::new_current_thread();
        builder.enable_time().build().expect("a runtime")
    })
}

/// The decode budget, one per process, so a reservation that leaks shows up in
/// the next input's [`settled`].
pub fn budget() -> Budget {
    static BUDGET: OnceLock<Budget> = OnceLock::new();
    let make = || Budget::new("fuzz", BUDGET_BYTES, Arc::new(SystemClock));
    BUDGET.get_or_init(make).clone()
}

/// Asserts everything reserved has been given back.
///
/// # Panics
/// When a reservation outlived its message: that is the finding.
pub fn settled(budget: &Budget) {
    assert_eq!(budget.used(), 0, "a reservation outlived its message");
}

/// The meter's bounds under the default configuration.
#[must_use]
pub fn limits() -> Limits {
    let config = Config::default();
    Limits {
        max_batch_bytes: norn_server::max_batch_bytes(&config),
        max_batch_samples: config.limits().max_batch_samples,
    }
}
