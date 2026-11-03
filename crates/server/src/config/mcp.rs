//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `[mcp]`: whether the daemon serves MCP, and on what. Off unless asked for: gRPC
//! on the local endpoint is always on, and a default daemon opens nothing else.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;

use super::ConfigError;

/// One way an agent reaches the daemon's MCP tools.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpTransport {
    /// One client on stdin and stdout, which started this daemon. Its leaving stops
    /// the daemon: the client is what the daemon was running for.
    Stdio,
    /// Streamable HTTP at `/mcp` on a loopback address. There is no authentication,
    /// so a non-loopback address is refused; a tunnel goes in front, named in `hosts`.
    Http(SocketAddr),
    /// Streamable HTTP at `/mcp` on a Unix socket, created owner-only.
    Unix(PathBuf),
    /// Streamable HTTP at `/mcp` on a named pipe, created owner-only.
    Pipe(String),
}

impl FromStr for McpTransport {
    type Err = String;

    /// `stdio`, `http:127.0.0.1:8090`, `unix:/path` or `pipe:\\.\pipe\name`.
    fn from_str(text: &str) -> Result<Self, String> {
        if text == "stdio" {
            return Ok(Self::Stdio);
        }
        match text.split_once(':') {
            Some(("http", address)) => {
                let address: SocketAddr = address.parse().map_err(|e| format!("{text}: {e}"))?;
                if !address.ip().is_loopback() {
                    return Err(format!("{text}: only a loopback address may serve MCP"));
                }
                Ok(Self::Http(address))
            }
            Some(("unix", path)) if !path.is_empty() => Ok(Self::Unix(path.into())),
            Some(("pipe", name)) if !name.is_empty() => Ok(Self::Pipe(name.to_owned())),
            _ => Err(format!(
                "{text}: expected stdio, http:ADDRESS:PORT, unix:PATH or pipe:NAME"
            )),
        }
    }
}

/// The `[mcp]` table. `Default` is off, with no transport. Plain data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpConfig {
    /// Whether MCP is served.
    pub enabled: bool,
    /// Where. Enabled with none named means [`McpTransport::Stdio`].
    pub transports: Vec<McpTransport>,
    /// Further `Host` names HTTP answers, beyond loopback's, such as a tunnel's.
    pub hosts: Vec<String>,
}

impl McpConfig {
    /// The transports to serve: none when off.
    #[must_use]
    pub fn served(&self) -> Vec<McpTransport> {
        match (self.enabled, self.transports.is_empty()) {
            (false, _) => Vec::new(),
            (true, true) => vec![McpTransport::Stdio],
            (true, false) => self.transports.clone(),
        }
    }
}

/// The `[mcp]` table as TOML spells it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mcp {
    enabled: Option<bool>,
    transports: Option<Vec<String>>,
    hosts: Option<Vec<String>>,
}

impl Mcp {
    /// This table, checked.
    pub(super) fn config(self) -> Result<McpConfig, ConfigError> {
        let transports = self.transports.unwrap_or_default();
        let transports = transports.iter().map(|t| t.parse());
        Ok(McpConfig {
            enabled: self.enabled.unwrap_or(false),
            transports: transports
                .collect::<Result<_, String>>()
                .map_err(|e| ConfigError::invalid("mcp.transports", e))?,
            hosts: self.hosts.unwrap_or_default(),
        })
    }
}
