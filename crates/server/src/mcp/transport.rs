//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! MCP's transports, as tpp-agents serves them. Each listener is one agent service
//! over the same [`Endpoint`], so every agent sees the same replay sessions.
//!
//! The runtime binds a Unix socket owner-only, replaces a stale one and refuses a
//! live one, and removes it on stop. Its `Host` guard answers loopback's names and
//! [`McpConfig::hosts`]; any other is refused (403), against DNS rebinding.

use std::sync::Arc;

use agents::error::ServiceError;
use agents::mcp::{DEFAULT_BASE_PATH, Service, Transport};
use agents::{Placement, Protocol, Ready, Requirements};
use tokio_util::sync::CancellationToken;

use super::dial::Dial;
use super::endpoint::Endpoint;
use crate::config::{McpConfig, McpTransport};

const LOOPBACK: &[&str] = &["localhost", "127.0.0.1", "::1", "[::1]"];

/// One agent service per transport `config` serves. Stdio's cancels `ended` when the
/// client closes stdin.
pub(crate) fn services(
    config: &McpConfig,
    dial: Dial,
    ended: &CancellationToken,
) -> Vec<Arc<dyn agents::Service>> {
    let endpoint = Endpoint::new(dial);
    let hosts: Vec<String> = LOOPBACK
        .iter()
        .map(|h| (*h).to_owned())
        .chain(config.hosts.iter().cloned())
        .collect();
    let http = |addr: String| -> Arc<dyn agents::Service> {
        Arc::new(
            Service::from_handler(endpoint.clone())
                .transports([Transport::StreamableHttp])
                .addr(addr)
                .base_path(DEFAULT_BASE_PATH)
                .allowed_hosts(hosts.clone()),
        )
    };
    let serve = |transport: McpTransport| -> Arc<dyn agents::Service> {
        match transport {
            McpTransport::Stdio => Arc::new(Stdio {
                inner: Service::from_handler(endpoint.clone()).transports([Transport::Stdio]),
                ended: ended.clone(),
            }),
            McpTransport::Http(address) => http(address.to_string()),
            McpTransport::Unix(path) => http(format!("unix:{}", path.display())),
            McpTransport::Pipe(name) => http(format!("pipe:{name}")),
        }
    };
    config.served().into_iter().map(serve).collect()
}

/// MCP over stdio, saying when its session ends. Correct from any task.
struct Stdio {
    inner: Service<Endpoint>,
    ended: CancellationToken,
}

#[async_trait::async_trait]
impl agents::Service for Stdio {
    fn protocol(&self) -> Protocol {
        Protocol::Mcp
    }

    fn requires(&self) -> Requirements {
        agents::Service::requires(&self.inner)
    }

    async fn serve(
        &self,
        cancel: CancellationToken,
        placement: Placement,
        ready: Arc<Ready>,
    ) -> Result<(), ServiceError> {
        let served = agents::Service::serve(&self.inner, cancel, placement, ready).await;
        self.ended.cancel();
        served
    }
}
