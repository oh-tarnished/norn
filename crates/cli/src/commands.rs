//! `status` and `version`, each a few public gRPC calls and their output.

use std::io::Write;

use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, GetServerRequest, Limits, NegotiateProtocolRequest};
use norn_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use tonic::transport::Channel;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;

use crate::CliError;

/// This client's name and release, sent as the negotiation `user_agent`.
pub const USER_AGENT: &str = concat!("nornctl/", env!("CARGO_PKG_VERSION"));

/// Prints the daemon's health (`grpc.health.v1`, the whole daemon) and its
/// `Server` resource. Needs no negotiation, so it works against any daemon.
///
/// # Errors
///
/// [`CliError::Call`] if either call fails; [`CliError::Output`].
pub async fn status(channel: Channel, out: &mut impl Write) -> Result<(), CliError> {
    let health = HealthClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await
        .map_err(|status| CliError::Call {
            method: "Health.Check",
            status,
        })?
        .into_inner();
    let health = ServingStatus::try_from(health.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    let server = AdminServiceClient::new(channel)
        .get_server(GetServerRequest {
            name: "server".to_owned(),
        })
        .await
        .map_err(|status| CliError::Call {
            method: "AdminService.GetServer",
            status,
        })?
        .into_inner();
    writeln!(out, "health: {health}")?;
    writeln!(out, "server_version: {}", server.server_version)?;
    writeln!(out, "protocol: {}", show(server.protocol_version))?;
    writeln!(out, "capabilities: {}", capabilities(&server.capabilities))?;
    if let Some(start) = server.start_time {
        writeln!(out, "start_time: {}.{:09}", start.seconds, start.nanos)?;
    }
    if let Some(limits) = server.limits {
        write_limits(out, &limits)?;
    }
    if let Some(r) = server.recovery {
        writeln!(out, "recovery.streams: {}", r.stream_count)?;
        writeln!(out, "recovery.events: {}", r.event_count)?;
        writeln!(out, "recovery.corruptions: {}", r.corruption_count)?;
        writeln!(
            out,
            "recovery.replayed_samples: {}",
            r.replayed_sample_count
        )?;
    }
    Ok(())
}

/// Prints this client's protocol version, then negotiates with the daemon
/// and prints what it agreed to (or fails with its refusal).
///
/// # Errors
///
/// [`CliError::Call`] with the daemon's reason, such as
/// `ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED`; [`CliError::Output`].
pub async fn version(channel: Channel, out: &mut impl Write) -> Result<(), CliError> {
    writeln!(out, "client: {USER_AGENT}")?;
    writeln!(
        out,
        "client_protocol: {}",
        show(Some(PROTOCOL_VERSION.into()))
    )?;
    let response = AdminServiceClient::new(channel)
        .negotiate_protocol(NegotiateProtocolRequest {
            name: "server".to_owned(),
            client_version: Some(PROTOCOL_VERSION.into()),
            required_capabilities: Vec::new(),
            user_agent: USER_AGENT.to_owned(),
        })
        .await
        .map_err(|status| CliError::Call {
            method: "AdminService.NegotiateProtocol",
            status,
        })?
        .into_inner();
    let agreed = response.protocol_version.ok_or(CliError::Malformed {
        method: "AdminService.NegotiateProtocol",
        field: "protocol_version",
    })?;
    writeln!(out, "server_version: {}", response.server_version)?;
    writeln!(out, "agreed_protocol: {}", show(Some(agreed)))?;
    Ok(())
}

fn show(v: Option<norn_protocol::admin::ProtocolVersion>) -> String {
    v.map(ProtocolVersion::from).map_or_else(
        || "unknown".to_owned(),
        |v| format!("{}.{}", v.major, v.minor),
    )
}

fn capabilities(numbers: &[i32]) -> String {
    if numbers.is_empty() {
        return "none".to_owned();
    }
    let names: Vec<String> = numbers
        .iter()
        .map(|n| {
            Capability::try_from(*n)
                .map(|c| c.as_str_name().to_owned())
                .unwrap_or_else(|_| n.to_string())
        })
        .collect();
    names.join(" ")
}

fn write_limits(out: &mut impl Write, l: &Limits) -> std::io::Result<()> {
    writeln!(out, "limits.max_batch_samples: {}", l.max_batch_samples)?;
    writeln!(out, "limits.max_batch_bytes: {}", l.max_batch_bytes)?;
    writeln!(out, "limits.max_pending_batches: {}", l.max_pending_batches)?;
    writeln!(out, "limits.max_streams: {}", l.max_streams)?;
    writeln!(out, "limits.max_replay_sessions: {}", l.max_replay_sessions)?;
    writeln!(out, "limits.max_subscriptions: {}", l.max_subscriptions)
}
