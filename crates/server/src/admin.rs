//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn.admin.v1.AdminService`: `GetServer` and `NegotiateProtocol`.

use std::sync::Arc;

use norn_protocol::admin::admin_service_server::AdminService;
use norn_protocol::admin::{
    Capability, CreateBackupRequest, GetServerRequest, MaintenanceReport, NegotiateProtocolRequest,
    NegotiateProtocolResponse, RetentionRemoval, Server, SharedMemoryPlane,
};
use norn_protocol::common::ErrorReason;
use norn_protocol::google::longrunning::Operation;
use norn_protocol::{PROTOCOL_VERSION, ProtocolVersion, SHARED_MEMORY_MINOR};
use norn_runtime::maintenance::{Maintenance, MaintenanceReport as Report};
use tonic::{Code, Request, Response, Status};

use crate::info::timestamp;
use crate::status::{bad_field, error_info, version_unsupported};

/// The longest `user_agent` written to the log; the rest is cut, so a
/// client cannot make one log record arbitrarily large.
const MAX_USER_AGENT: usize = 128;

/// The AdminService implementation: a fixed description of this daemon, and what
/// maintenance has done since it started.
///
/// Concurrency: immutable after construction; tonic shares it across every
/// connection and call. Neither method allocates beyond its response.
#[derive(Debug, Clone)]
pub struct Admin {
    server: Server,
    maintenance: Option<Arc<Maintenance>>,
    shared_memory: Option<SharedMemoryPlane>,
}

impl Admin {
    /// Serves `server`, built by [`server_resource`](crate::server_resource).
    #[must_use]
    pub fn new(server: Server) -> Self {
        Self {
            server,
            maintenance: None,
            shared_memory: None,
        }
    }

    /// Offers the shared-memory data plane `plane` describes: adds
    /// `CAPABILITY_SHARED_MEMORY`, and tells clients agreeing 1.7 or later how to
    /// attach. `None` changes nothing.
    #[must_use]
    pub fn shared_memory(mut self, plane: Option<SharedMemoryPlane>) -> Self {
        if plane.is_some() {
            self.server
                .capabilities
                .push(Capability::SharedMemory as i32);
        }
        self.shared_memory = plane;
        self
    }

    /// Reports what `maintenance` has done in `GetServer`.
    #[must_use]
    pub fn maintenance(mut self, maintenance: Arc<Maintenance>) -> Self {
        self.maintenance = Some(maintenance);
        self
    }
}

fn report_to_wire(r: Report) -> MaintenanceReport {
    let wide = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
    let removals = r.recent.iter().map(|removal| RetentionRemoval {
        stream: format!("streams/{}", removal.stream),
        remove_time: Some(timestamp(removal.at)),
        segment_count: wide(removal.segments),
        sample_count: wide(removal.samples),
        removed_bytes: wide(removal.bytes),
    });
    MaintenanceReport {
        compaction_count: wide(r.compactions),
        compacted_segment_count: wide(r.compacted_segments),
        compaction_saved_bytes: r.saved_bytes,
        compaction_failure_count: wide(r.failures),
        retention_segment_count: wide(r.retention_segments),
        retention_sample_count: wide(r.retention_samples),
        retention_bytes: wide(r.retention_bytes),
        recent_removals: removals.collect(),
    }
}

fn check_name(name: &str) -> Result<(), Status> {
    if name == "server" {
        Ok(())
    } else {
        Err(bad_field("name", "must be `server`"))
    }
}

#[tonic::async_trait]
impl AdminService for Admin {
    async fn get_server(&self, req: Request<GetServerRequest>) -> Result<Response<Server>, Status> {
        check_name(&req.get_ref().name)?;
        let mut server = self.server.clone();
        server.maintenance = self
            .maintenance
            .as_ref()
            .map(|m| report_to_wire(m.report()));
        Ok(Response::new(server))
    }

    async fn create_backup(
        &self,
        req: Request<CreateBackupRequest>,
    ) -> Result<Response<Operation>, Status> {
        let maintenance = self.maintenance.as_ref().ok_or_else(|| {
            Status::unavailable("this daemon was started without its maintenance worker")
        })?;
        crate::backup::start(maintenance, req.into_inner()).map(Response::new)
    }

    async fn negotiate_protocol(
        &self,
        req: Request<NegotiateProtocolRequest>,
    ) -> Result<Response<NegotiateProtocolResponse>, Status> {
        let req = req.into_inner();
        check_name(&req.name)?;
        let Some(client) = req.client_version.map(ProtocolVersion::from) else {
            return Err(bad_field("client_version", "is required"));
        };
        let agent: String = req.user_agent.chars().take(MAX_USER_AGENT).collect();
        let Some(agreed) = PROTOCOL_VERSION.negotiate(client) else {
            tracing::info!(%agent, client.major, "refused protocol version");
            return Err(version_unsupported(format_args!(
                "{}.{}",
                client.major, client.minor
            )));
        };
        let offered = &self.server.capabilities;
        if let Some(missing) = req
            .required_capabilities
            .iter()
            .find(|c| !offered.contains(c))
        {
            let name = Capability::try_from(*missing)
                .map(|c| c.as_str_name().to_owned())
                .unwrap_or_else(|_| missing.to_string());
            tracing::info!(%agent, capability = %name, "required capability unavailable");
            return Err(error_info(
                Code::FailedPrecondition,
                ErrorReason::CapabilityUnavailable,
                format!("capability {name} is not offered by this daemon"),
                [("capability", name)],
            ));
        }
        tracing::debug!(%agent, agreed.major, agreed.minor, "negotiated protocol");
        // A client before 1.7 does not know the field: it is not sent.
        let shared_memory = (agreed.minor >= SHARED_MEMORY_MINOR)
            .then(|| self.shared_memory.clone())
            .flatten();
        Ok(Response::new(NegotiateProtocolResponse {
            protocol_version: Some(agreed.into()),
            capabilities: offered.clone(),
            limits: self.server.limits,
            server_version: self.server.server_version.clone(),
            shared_memory,
        }))
    }
}
