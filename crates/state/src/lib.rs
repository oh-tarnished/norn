//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's state reconstruction (SPEC §23, §26): what a system looked like at T.
//!
//! # Layer contract
//!
//! - **Owns**: [`StateReader`], which answers `StateAt(T)` for a list of signals,
//!   and [`Pins`], which holds an answer so that all its pages describe one state.
//!   docs/reference/query.md, "State at an instant", is the design.
//! - **Must not**: know the wire protocol, the runtime or the write path. Read a
//!   clock (the time arrives as an argument), or leave a thread behind: a read's
//!   scoped threads are joined before it returns. Expose checkpoints
//!   or chunks: callers name an instant and nothing else (SPEC §26).
//! - **May import** `norn-core`, `norn-storage`, `norn-index`, `norn-query`. It uses
//!   only the first and last. No third-party crate.
//! - **Spans**: none yet; [`State::stats`] counts what an answer read.
//!
//! # The invariant
//!
//! `StateAt(T)` equals what folding every selected sample at or before `T`, in
//! (time, sequence) order, leaves behind (SPEC §36). `tests/properties.rs` proves it
//! against a brute-force scan and against a replay-ordered query, with buffered
//! samples and overlapping chunks.
//!
//! # Concurrency
//!
//! [`StateReader`] and [`Pins`] are `Send + Sync` and shared. A read blocks on the
//! disk, so call it from a blocking context.
//!
//! # Failure
//!
//! A chunk failing to read is [`QueryError::Corrupt`], never "no state". A race with
//! compaction or retention is handled underneath, invisibly.
//!
//! # Memory
//!
//! An answer is one entry a selected signal, charged to the engine's memory before
//! it is held and for as long as it is held. [`Pins`] holds a bounded number of them
//! for a bounded time.
//!
//! [`QueryError::Corrupt`]: norn_query::QueryError::Corrupt

#![forbid(unsafe_code)]

mod pins;
mod reader;

pub use pins::{PinId, Pins, page_length};
pub use reader::{SignalState, State, StateReader};
