//! Norn's query engine (SPEC §22): ranges and aggregates, in bounded memory.
//!
//! # Layer contract
//!
//! - **Owns**: reading samples back. Filters ([`Filter`]), the time slices that
//!   keep a read inside its budget, the merge of stored chunks with the write
//!   path's buffers, reductions over fixed windows, resume positions, and the
//!   bounded cache of open segment files. docs/reference/query.md is the design.
//! - **Must not**: know the wire protocol, gRPC or the runtime. It cannot name a
//!   `Budget` or the write path's buffers, so both arrive through traits this
//!   crate declares: [`QueryMemory`] and [`TailSource`]. It reads no clock and
//!   spawns nothing, and it writes nothing but what `TemporalIndex::refresh` does.
//! - **May import** `norn-core`, `norn-codec`, `norn-storage`, `norn-index`. No
//!   third-party crate.
//! - **Spans**: none yet; a [`Query`] counts what it did in [`QueryStats`].
//!
//! # Concurrency
//!
//! An [`Engine`] is `Send + Sync` and shared. A [`Query`] is `Send`, owned by one
//! caller at a time, and pulled with [`Query::next_page`], which blocks on the
//! disk: call it from a blocking context. Between pages it holds no thread, so a
//! consumer that stops reading stalls only its own query.
//!
//! # Failure
//!
//! Nothing panics on stored data. A chunk failing its checksum stops the query
//! with [`QueryError::Corrupt`], never a skip. What was delivered before it is
//! exact. A race with compaction or retention is handled here, invisibly.
//!
//! # Memory
//!
//! Everything held is charged to [`QueryMemory`] before it is held: rows, one
//! decoded chunk, pages not yet delivered, open files' buffers, signal names.
//! Results are never built whole. [`QueryLimits`] holds the bounds.

#![forbid(unsafe_code)]

mod aggregate;
mod engine;
mod error;
mod files;
mod filter;
mod latest;
mod limits;
mod memory;
mod newest;
mod page;
mod query_run;
mod read;
mod resume;
mod rows;
mod selection;
mod slice;
mod spec;
mod tail;
mod token;
mod walk;

pub use aggregate::Aggregate;
pub use engine::{Engine, QueryStats};
pub use error::{ChunkDamage, QueryError, Result};
pub use filter::{Filter, FilterError};
pub use limits::QueryLimits;
pub use memory::{Charge, QueryMemory, Refused};
pub use newest::Newest;
pub use page::{Page, Results};
pub use query_run::Query;
pub use spec::{Function, Order, Output, QuerySpec, Reduction, Target, TimeRange};
pub use tail::{NoTail, TailSource};
pub use token::Position;
