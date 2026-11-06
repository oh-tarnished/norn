//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The checks made before a database is touched.

use norn_ipc::{Server, ServerConfig};

use super::DaemonError;
use crate::config::DaemonConfig;

/// Claims and releases each local endpoint, the daemon's and every MCP one: a live
/// daemon, a squatter or an insecure directory fails here with its own error. The
/// runtime that binds them for real uses an existing directory as it finds it, so
/// this is where the owner-only rule of docs/reference/ipc.md is enforced. Says in
/// the log what is served to every local user.
pub(super) async fn claim_endpoints(config: &DaemonConfig) -> Result<(), DaemonError> {
    drop(Server::bind(&config.endpoint(), ServerConfig::default()).await?);
    for endpoint in config.mcp_settings().local_endpoints() {
        drop(Server::bind(&endpoint, ServerConfig::default()).await?);
    }
    for exposure in config.exposures() {
        tracing::warn!("{exposure}");
    }
    Ok(())
}
