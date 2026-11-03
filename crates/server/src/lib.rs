//! The Norn daemon: configuration, AdminService and health over local IPC.
//!
//! # Layer contract
//!
//! - **Owns**: serving through runtime-rs's `HybridServer`, gRPC always and MCP
//!   when `[mcp]` asks, its tools generated from the schema ([`read_only_tool_names`]);
//!   loading the daemon's configuration (TOML file and flags over
//!   norn-core's defaults, [`DaemonConfig`]); the [`Daemon`], which binds a
//!   local endpoint and serves gRPC as a task of a `norn-runtime`
//!   [`Runtime`](norn_runtime::Runtime); `AdminService` ([`Admin`]);
//!   `grpc.health.v1` mirroring the runtime's health; `EntityService`,
//!   `SignalService` and `StreamService` over the registry and the maintenance
//!   worker ([`Catalog`]); `google.longrunning.Operations` ([`OperationsService`]);
//!   `AppendSamples` over the write path ([`Telemetry`]); `QueryService`
//!   over `norn-query`, reached through the runtime ([`Queries`]); `StateService`
//!   over `norn-state`, the same way ([`States`]); `ReplayService` over the
//!   runtime's sessions ([`ReplaySessions`]); `SubscribeEvents`, live and replayed
//!   through one event model ([`Subscriptions`]); wire-to-core
//!   conversions ([`convert`]); charging decode memory to the IPC budget
//!   ([`MeterLayer`]); `UNIMPLEMENTED` stubs, with AIP-193 error details, for
//!   every service not built yet;
//!   the `norn-protocol-version` check ([`VersionGate`]); shutdown signals;
//!   and bounded structured logging. The `norn` binary is
//!   a thin `main` over this library.
//! - **Must not**: reach storage except through the runtime, or let a
//!   client see anything but the public protocol. Platform code lives only
//!   in `signal/`.
//! - **May import**: `norn-core`, `norn-protocol`, `norn-transport`,
//!   `norn-ipc`, `norn-shared-memory`, `norn-metadata`, `norn-runtime`;
//!   third-party: `tpp-grpc`, `tpp-agents`, `rmcp`, `prost-reflect`, `async-trait`,
//!   `axum`, `serde_json`, `tonic`, `tonic-health`, `prost`, `prost-types`, `tokio`,
//!   `tokio-util`, `tower`, `http`, `http-body`, `bytes`, `tracing`,
//!   `tracing-subscriber`, `serde`, `toml`.
//! - **Spans**: none yet. Events: `ipc drained`, `shutdown requested`,
//!   protocol refusals at `info`, and the runtime's lifecycle events.
//!
//! Concurrency: [`Admin`] and the stubs are immutable and shared by tonic
//! across connections. Shutdown: cancelling the token passed to
//! [`Daemon::run`] reports `NOT_SERVING`, stops accepting, drains in-flight
//! calls up to the drain bound, aborts the rest, removes the socket, and
//! returns once every task has stopped.

mod admin;
mod catalog;
pub mod config;
pub mod convert;
mod daemon;
mod events;
mod health;
mod info;
pub mod logging;
mod mcp;
mod metered;
mod opening;
mod operations;
mod query;
mod replay;
mod signal;
mod state;
mod status;
mod telemetry;
mod unimplemented;
mod version_gate;

pub use admin::Admin;
pub use catalog::Catalog;
pub use config::{
    ConfigError, DaemonConfig, LogConfig, LogFormat, LogLevel, McpConfig, McpTransport,
};
pub use daemon::{Daemon, DaemonError};
pub use events::{Events, Subscriptions};
pub use health::serving_status;
pub use info::{MAX_MESSAGE_BYTES, SERVER_VERSION, capabilities, max_batch_bytes, server_resource};
pub use mcp::read_only_tool_names;
pub use metered::{Meter, MeterLayer};
pub use operations::OperationsService;
pub use query::Queries;
pub use replay::ReplaySessions;
pub use signal::shutdown_requested;
pub use state::States;
pub use status::ERROR_DOMAIN;
pub use telemetry::Telemetry;
pub use unimplemented::{Stubbed, Unimplemented};
pub use version_gate::{VersionGate, VersionGateLayer};
