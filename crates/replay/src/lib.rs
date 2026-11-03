//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's replay (SPEC §25, §27, §28): history, run on a virtual clock.
//!
//! # Layer contract
//!
//! - **Owns**: [`VirtualClock`]; [`Session`], the state machine of one replay; the
//!   [`Event`] model live and replayed telemetry share; and the [`ReplayError`]s a
//!   session can give. docs/reference/replay.md is the design.
//! - **Must not**: know the wire protocol, read a clock or spawn anything. A session
//!   is told the wall time and answers with a decision and a deadline ([`Next`]);
//!   `norn-runtime` owns the task, the clock and the
//!   subscribers. That is what lets a whole scenario run with no timer.
//! - **May import** `norn-core`, `norn-storage`, `norn-index`, `norn-query`,
//!   `norn-state`. It uses the first, and the last two. No third-party crate.
//! - **Spans**: none yet.
//!
//! # Determinism
//!
//! The sample events of a session, and their order, depend only on its interval,
//! its signals, and the seeks and steps applied: never on speed, pauses or how fast
//! anyone reads. A sample event's `clock_time` is its own `observe_time`.
//!
//! # Concurrency
//!
//! A [`Session`] is `Send` and owned by one driver. Its reads block on the disk.
//!
//! # Failure
//!
//! A damaged chunk stops the read with [`ReplayError::Query`], the session pauses
//! before it, and nothing already emitted is wrong: a slice is read whole before any
//! of it is emitted. A race with compaction costs a re-read, not an event.
//!
//! # Memory
//!
//! A session holds one page of samples and one query's files and rows, all charged
//! to the memory of the engine it was given. A batch keeps its page's charge alive
//! until every subscriber has it, so unread events are never unaccounted.

#![forbid(unsafe_code)]

mod clock;
mod cursor;
mod error;
mod event;
mod session;

pub use clock::{Pace, VirtualClock};
pub use error::ReplayError;
pub use event::{Batch, ClockKind, Event, MAX_BATCH_EVENTS, Payload};
pub use session::{Control, Next, ReplayState, Session, SessionSpec, SessionView};
