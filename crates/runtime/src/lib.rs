//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's daemon lifecycle: health, memory budgets and task ownership.
//!
//! # Layer contract
//!
//! - **Owns**: the [`Runtime`] lifecycle (`new` spawns nothing, `run`
//!   blocks and owns the shutdown of every task it spawns, `close` is
//!   idempotent); [`Health`] (starting, serving, draining, stopped) and a
//!   [`HealthWatch`] on it; the per-subsystem memory [`Budget`]s drawn from
//!   `Config::memory()`, with typed refusal and bounded waiting as
//!   backpressure (SPEC §16, §18); and the injected [`Clock`], so every
//!   timeout here runs on a [`FakeClock`] in tests.
//! - **Must not**: know networks, sockets, gRPC or the protocol. The server
//!   crate serves gRPC as one of the runtime's tasks. It must not read the
//!   wall clock except through [`SystemClock`].
//!   It owns the write path too ([`ingest`]): admission, journal, group commit,
//!   per-signal buffers, recovery; docs/reference/storage.md, "Write path".
//!   It implements `norn-query`'s seams: the buffers as a `TailSource`, so a query
//!   sees every persisted sample, and the query [`Budget`] as a `QueryMemory`.
//!   It drives replay sessions on its clock and tasks ([`sessions`]), and fans
//!   committed samples out to live subscribers without ever waiting ([`live`]).
//! - **May import**: `norn-core`, `norn-metadata`, `norn-storage`, `norn-codec`,
//!   `norn-index`, `norn-query`, `norn-state`, `norn-replay`; third-party:
//!   `tokio`, `tokio-util`, `tracing`.
//! - **Spans**: none yet; lifecycle transitions and task failures are
//!   `tracing` events (`runtime serving`, `runtime draining`, `runtime
//!   stopped`, `task failed`, `task panicked`).
//!
//! Concurrency: `Runtime`, `Budget` and `HealthWatch` are `Send + Sync` and
//! meant to be shared. Nothing here panics; misuse is a [`RuntimeError`].
//!
//! ```
//! use norn_core::Config;
//! use norn_runtime::{Health, Runtime};
//! use tokio_util::sync::CancellationToken;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), norn_runtime::RuntimeError> {
//! let runtime = Runtime::new(Config::default())?; // spawns nothing
//! runtime.add_task("idle", |token| async move {
//!     token.cancelled().await; // finish in-flight work here
//!     Ok(())
//! })?;
//! let cancel = CancellationToken::new();
//! cancel.cancel(); // normally a signal handler does this
//! let report = runtime.run(cancel).await?;
//! assert_eq!(report.aborted, 0);
//! assert_eq!(runtime.health().get(), Health::Stopped);
//! # Ok(()) }
//! ```

mod budget;
mod clock;
mod error;
mod health;
pub mod ingest;
pub mod live;
pub mod maintenance;
mod run;
mod runtime;
pub mod sessions;
mod subscription;

/// The query engine, for the server, which may import this crate and not that
/// one: the layer table keeps the daemon's services above the data plane.
pub use norn_query as query;
/// Replay's sessions and events, for the same reason.
pub use norn_replay as replay;
/// State reconstruction, for the same reason.
pub use norn_state as state;
/// The stored files, for the same reason: their keys and their directory's mode.
pub use norn_storage as storage;

pub use budget::{Budget, BudgetError, Budgets, Reservation, Subsystem};
pub use clock::{Clock, FakeClock, Sleep, SystemClock};
pub use error::{RuntimeError, TaskError};
pub use health::{Health, HealthWatch};
pub use run::RunReport;
pub use runtime::{DEFAULT_SHUTDOWN_TIMEOUT, MAX_TASKS, Runtime};
pub use subscription::{End, Subscription};
