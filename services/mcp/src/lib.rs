//! `norn-mcp`: an MCP server giving AI agents Norn's temporal primitives.
//!
//! # Layer contract
//!
//! - **Owns**: the MCP tools of SPEC §31, their schemas and descriptions, bounded
//!   results and continuations ([`server`], `tools`), and serving them over stdio or
//!   loopback streamable HTTP ([`http`]).
//! - **Must not**: touch storage or any part of the daemon but its public gRPC API
//!   (SPEC §31, §47); write to Norn; reason. It supplies deterministic temporal
//!   primitives and no inference (SPEC §32). A replay session is the only thing it
//!   creates in the daemon, and it stops those itself.
//! - **May import**: `norn-protocol`, `norn-ipc`; third-party: `rmcp`, `schemars`,
//!   `serde`, `serde_json`, `tonic`, `prost-types`, `tokio`, `tokio-util`, `hyper-util`,
//!   `tower`, `http`, `http-body-util`, `bytes`, `tracing-subscriber`.
//! - **Spans**: none. Logs go to stderr, since stdout carries the stdio transport.
//!
//! Lifecycle: [`Daemon::new`] and [`NornMcp::new`] do no I/O and spawn nothing, so a
//! daemon that is down costs a tool error and not a failed start. [`run`] blocks until
//! the client leaves or the process is told to stop, and stops every task first.
//!
//! Bounds: rows and bytes per result (`page`), one value (`render`), samples scanned
//! per call, calls at once, connections, replay sessions, and a deadline on each call.

mod args;
mod cursor;
mod daemon;
mod error;
mod hints;
pub mod http;
mod page;
mod render;
mod run;
mod select;
pub mod server;
mod signal_scan;
mod time;
mod tools;
mod walk;

pub use args::{Args, Transport, USAGE, parse};
pub use daemon::Daemon;
pub use error::ToolError;
pub use run::run;
pub use server::NornMcp;
pub use tools::tools;
