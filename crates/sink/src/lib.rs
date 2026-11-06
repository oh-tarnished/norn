//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The protocol-neutral half of a live adapter: everything between "a reading
//! arrived" and "Norn acknowledged it", so a new protocol writes only its codec
//! and a mapping onto entities and signals.
//!
//! # Layer contract
//!
//! - **Owns**: [`Sink`], one stream's delivery into a daemon: the bounded byte
//!   buffer with a per-signal share; the readiness gate, so a reading waits only
//!   for its own signal; provisioning, by AIP-233 batch create cut to the
//!   message limit, halved on `RESOURCE_EXHAUSTED` and per item on
//!   `UNIMPLEMENTED`, coalesced and idempotent across restarts; the
//!   `AppendSamples` pump with SPEC §18's four outcomes, SPEC §19's levels,
//!   reconnection with backoff and resend of what was not acknowledged; the
//!   daemon's drain; reachability and health; [`endpoint`] resolution; and,
//!   with the `shared-memory` feature, the ring through `norn_ipc::AppendCall`.
//! - **Must not**: know any protocol, or any word of one. A protocol's
//!   vocabulary travels in annotations, which Norn indexes and never reads
//!   (SPEC §4, §30). Reach the daemon but through the public API (SPEC §47).
//!   Be imported by the engine or the daemon: it is a client crate
//!   (`xtask/src/arch.rs`). Hold anything without a bound (SPEC §16), or drop a
//!   reading without counting it (SPEC §18).
//! - **May import**: `norn-protocol`, `norn-ipc`, `norn-transport`, and with
//!   the `shared-memory` feature `norn-shared-memory`; third-party: `tonic`,
//!   `tokio`, `prost`, `prost-types`.
//! - **Spans**: none; [`Notice`]s say what a log would.
//! - **Tasks**: [`Sink::new`] spawns nothing. [`Sink::run`] is the one loop
//!   that talks to the daemon; the caller runs it, and it returns only after
//!   [`Sink::close`] and the drain, with every task it started stopped.
//!
//! The vocabulary is `norn-protocol`'s ([`Kind`], [`ValueType`], [`Quality`]),
//! as in `norn-adapter`: a reading holds what the wire holds.
//!
//! ```no_run
//! use norn_sink::{Catalog, Entity, Kind, Options, Reading, Signal, Sink, ValueType};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let (endpoint, _rule) = norn_sink::endpoint::locate(None)?;
//! let sink = Sink::new(endpoint, "plant", Options::default())?;
//! let running = tokio::spawn({
//!     let sink = sink.clone();
//!     async move { sink.run().await }
//! });
//! let catalog = Catalog::new()
//!     .entity(Entity::new("pump-01").display_name("Pump 1"))
//!     .signal(Signal::new("pump-01", "speed", ValueType::Double).unit("rpm"));
//! sink.declare(&catalog)?;
//! let at = std::time::SystemTime::now();
//! sink.push([Reading::new("pump-01", "speed", at, Kind::DoubleValue(1450.0))])?;
//! sink.close().await;
//! let report = running.await??;
//! println!("{} acknowledged", report.counts.acknowledged);
//! # Ok(()) }
//! ```

mod catalog;
mod counts;
pub mod endpoint;
mod error;
mod flow;
mod health;
pub mod ids;
mod join;
mod options;
mod provision;
mod pump;
mod reading;
mod shared;
mod sink;

pub use catalog::{Catalog, Entity, Signal};
pub use counts::Counts;
pub use error::SinkError;
pub use health::{Health, Notice, Reach, Report};
pub use options::Options;
pub use reading::Reading;
pub use sink::{Declared, Pushed, Sink};

pub use norn_ipc::Endpoint;
pub use norn_protocol::common::value::Kind;
pub use norn_protocol::common::{EnumValue, Quality, QualitySeverity, QualitySubstatus, ValueType};
pub use norn_protocol::telemetry::AcknowledgementLevel;
