//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Learning, over the control plane, whether and how shared memory is offered.

use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, NegotiateProtocolRequest, SharedMemoryPlane};
use norn_protocol::{PROTOCOL_VERSION, ProtocolVersion, SHARED_MEMORY_MINOR};
use tonic::transport::Channel;

/// What this client calls itself in `NegotiateProtocol`, for the daemon's log.
const USER_AGENT: &str = concat!("norn-ipc/", env!("CARGO_PKG_VERSION"));

/// The daemon's shared-memory plane, or `None` to stay on gRPC: negotiation
/// failed (the append call that follows reports anything real), the agreed
/// version predates 1.7, or the daemon does not offer the capability.
pub(super) async fn shared_memory(channel: Channel) -> Option<SharedMemoryPlane> {
    let request = NegotiateProtocolRequest {
        name: "server".to_owned(),
        client_version: Some(PROTOCOL_VERSION.into()),
        required_capabilities: Vec::new(),
        user_agent: USER_AGENT.to_owned(),
    };
    let reply = AdminServiceClient::new(channel)
        .negotiate_protocol(request)
        .await
        .ok()?
        .into_inner();
    let agreed = ProtocolVersion::from(reply.protocol_version?);
    let offered = reply
        .capabilities
        .contains(&(Capability::SharedMemory as i32));
    let current = agreed.major == PROTOCOL_VERSION.major && agreed.minor >= SHARED_MEMORY_MINOR;
    reply.shared_memory.filter(|_| offered && current)
}
