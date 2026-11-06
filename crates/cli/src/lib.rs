//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl`, the Norn command-line client.
//!
//! # Layer contract
//!
//! - **Owns**: the `nornctl` commands: [`status`] (health and the `Server`
//!   resource), [`version`] (protocol negotiation), [`resources`] (streams,
//!   entities, signals, append, query), and their command line ([`parse`]).
//! - **Must not**: reach the daemon any way but the public gRPC API over
//!   norn-ipc (SPEC §33, §47). It never imports the runtime, storage or the
//!   server, except the server as a dev-dependency for integration tests.
//!   The exception that is not a client at all: [`offline`], `restore` and
//!   `migrate`, whose whole point is that no daemon runs, go through
//!   `norn-database`, and that alone.
//! - **May import**: `norn-core`, `norn-protocol`, `norn-transport`,
//!   `norn-ipc`, `norn-database`, and the adapters over them (`norn-adapter`, `norn-adapter-csv`,
//!   `norn-adapter-parquet`, `norn-adapter-mcap`), which are clients too;
//!   `norn-shared-memory`, only to attach a granted ring when `append` or
//!   `import` is given `--shared-memory`; third-party: `tonic`, `tonic-health`,
//!   `prost-types`, `tokio`.
//! - **Spans**: none; a CLI prints, it does not trace.
//!
//! Every command connects with [`connect`], which bounds both connecting
//! and each call by the `--timeout`, so `nornctl` never hangs on a wedged
//! daemon. Output is `key: value` lines on stdout, stable enough to grep;
//! errors are one line on stderr and exit status 1 (2 for usage).

mod append;
mod args;
mod backup;
mod benchmark;
mod commands;
mod error;
mod inspect;
pub mod offline;
mod plane;
mod query;
mod rekey;
mod replay;
pub mod resources;
mod retention;
mod session;
mod state;
mod stats;
mod subscribe;
mod time;
mod transfer;
mod upkeep;
mod verify;

use std::time::Duration;

use norn_ipc::{Endpoint, connect_with};
use tonic::transport::Channel;

pub use args::{Args, Command, DEFAULT_TIMEOUT, USAGE, parse};
pub use commands::{USER_AGENT, status, version};
pub use error::CliError;
pub use rekey::rekey;

/// The HTTP/2 authority sent to the daemon. Unused for routing (the
/// endpoint decides where bytes go); `.invalid` never resolves (RFC 2606).
const AUTHORITY: &str = "http://norn.invalid";

/// Connects to `endpoint`, bounding the connection and every call on the
/// returned channel by `timeout`.
///
/// # Errors
///
/// [`CliError::Connect`] when no daemon answers within `timeout`.
pub async fn connect(endpoint: &Endpoint, timeout: Duration) -> Result<Channel, CliError> {
    let template = tonic::transport::Endpoint::from_static(AUTHORITY)
        .connect_timeout(timeout)
        .timeout(timeout);
    connect_with(endpoint, template)
        .await
        .map_err(|source| CliError::Connect {
            endpoint: endpoint.clone(),
            source,
        })
}
