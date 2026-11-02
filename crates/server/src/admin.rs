//! `norn.admin.v1.AdminService`: `GetServer` and `NegotiateProtocol`.

use norn_protocol::admin::admin_service_server::AdminService;
use norn_protocol::admin::{
    Capability, GetServerRequest, NegotiateProtocolRequest, NegotiateProtocolResponse, Server,
};
use norn_protocol::common::ErrorReason;
use norn_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use tonic::{Code, Request, Response, Status};

use crate::status::{bad_field, error_info, version_unsupported};

/// The longest `user_agent` written to the log; the rest is cut, so a
/// client cannot make one log record arbitrarily large.
const MAX_USER_AGENT: usize = 128;

/// The AdminService implementation: a fixed description of this daemon.
///
/// Concurrency: immutable after construction; tonic shares it across every
/// connection and call. Neither method allocates beyond its response.
#[derive(Debug, Clone)]
pub struct Admin {
    server: Server,
}

impl Admin {
    /// Serves `server`, built by [`server_resource`](crate::server_resource).
    #[must_use]
    pub fn new(server: Server) -> Self {
        Self { server }
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
        Ok(Response::new(self.server.clone()))
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
        Ok(Response::new(NegotiateProtocolResponse {
            protocol_version: Some(agreed.into()),
            capabilities: offered.clone(),
            limits: self.server.limits,
            server_version: self.server.server_version.clone(),
        }))
    }
}
