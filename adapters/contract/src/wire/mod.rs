//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The drivers' calls to the daemon, all through the public API.

pub(crate) mod appender;
pub(crate) mod listing;
pub(crate) mod present;
mod refused;
pub(crate) mod registry;

use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, Limits, NegotiateProtocolRequest, ProtocolVersion};
use tonic::transport::Channel;

use crate::AdapterError;

/// Negotiates the protocol, requiring `needed`, and returns the published limits.
pub(crate) async fn negotiate(
    channel: Channel,
    needed: &[Capability],
) -> Result<Limits, AdapterError> {
    const METHOD: &str = "AdminService.NegotiateProtocol";
    let version = norn_protocol::PROTOCOL_VERSION;
    let request = NegotiateProtocolRequest {
        name: "server".to_owned(),
        client_version: Some(ProtocolVersion {
            major: version.major,
            minor: version.minor,
        }),
        required_capabilities: needed.iter().map(|c| *c as i32).collect(),
        user_agent: concat!("norn-adapter/", env!("CARGO_PKG_VERSION")).to_owned(),
    };
    let reply = AdminServiceClient::new(channel)
        .negotiate_protocol(request)
        .await
        .map_err(AdapterError::call(METHOD))?;
    reply.into_inner().limits.ok_or(AdapterError::Response {
        method: METHOD,
        detail: "limits",
    })
}
