//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Standard schema discovery on owner-only Unix sockets and named pipes.

use norn_ipc::Endpoint;
use norn_protocol::FILE_DESCRIPTOR_SET;
use tonic_reflection::server::Builder;
use tpp_grpc::HybridServer;

use super::DaemonError;

pub(super) fn register(
    server: HybridServer,
    endpoint: &Endpoint,
) -> Result<HybridServer, DaemonError> {
    if !matches!(endpoint, Endpoint::Unix(_) | Endpoint::Pipe(_)) {
        return Ok(server);
    }
    let builder = || {
        Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
    };
    let v1 = builder().build_v1().map_err(DaemonError::Reflection)?;
    let alpha = builder().build_v1alpha().map_err(DaemonError::Reflection)?;
    Ok(server.with_service(v1).with_service(alpha))
}
