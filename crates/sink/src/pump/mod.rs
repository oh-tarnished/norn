//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The one loop that talks to the daemon, from protocol.md's "Writing a
//! client": connect, negotiate, create the stream, keep one `AppendSamples`
//! call open, and when any of it fails, wait and start again. A daemon going
//! away is the expected case here, so it is an arm of this loop.

use crate::counts::Backoff;
use crate::health::{Notice, Report};
use crate::provision::Failure;
use crate::shared::Shared;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{Capability, NegotiateProtocolRequest};
use norn_protocol::telemetry::AcknowledgementLevel;
use norn_protocol::{PROTOCOL_VERSION, admin};
use std::time::Duration;
use tokio::time::{Instant, sleep};
use tonic::transport::{Channel, Endpoint};

/// Frames on a quiet connection: a daemon closes one silent for 10 minutes.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

mod limits;
mod open;
mod plane;
mod provide;
mod session;
#[cfg(test)]
mod tests;

/// How a session ended.
pub(crate) enum End {
    /// Everything went, or the drain ran out: the run is over.
    Finished,
    /// The daemon is away; why.
    Lost(String),
    /// The daemon is draining to stop (protocol 1.7): it answered every batch
    /// it admitted and ended the call. Norn going away on purpose, not a
    /// failure: what it did not acknowledge is resent on a new call, and the
    /// sink's reach does not turn unless the new call cannot be had.
    Draining,
}

impl From<Failure> for End {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Lost(why) | Failure::Refused(why) => Self::Lost(why),
        }
    }
}

pub(crate) fn lost(e: impl std::fmt::Display) -> End {
    End::Lost(e.to_string())
}

/// How a call ending with `status` ends the session: [`End::Draining`] for
/// `UNAVAILABLE` with `ERROR_REASON_SHUTTING_DOWN` in its `ErrorInfo`, which
/// Norn from 1.0.0-rc.1 ends an open `AppendSamples` with once it has answered
/// what it admitted (protocol.md, "Ingestion"); else lost.
pub(crate) fn ended(status: &tonic::Status) -> End {
    if draining(status) {
        End::Draining
    } else {
        lost(status.message())
    }
}

/// Whether `status` is Norn's drain. Matched on the reason's name, never on
/// the message.
pub(crate) fn draining(status: &tonic::Status) -> bool {
    use norn_protocol::common::ErrorReason;
    use norn_protocol::rpc::StatusExt;
    status.code() == tonic::Code::Unavailable
        && status
            .get_error_details()
            .error_info()
            .is_some_and(|info| info.reason == ErrorReason::ShuttingDown.as_str_name())
}

/// Runs until the sink closes and what it holds has gone or been given up.
/// Never fails: everything that goes wrong here is reported.
pub(crate) async fn run(shared: &Shared) -> Report {
    if shared.options.shared_memory && !plane::AVAILABLE {
        shared.tell(&Notice::NoSharedMemory);
    }
    let origin = Instant::now();
    let mut backoff = Backoff::default();
    loop {
        let was_closing = shared.is_closing();
        match session::session(shared, origin, &mut backoff).await {
            End::Finished => break,
            End::Lost(why) => {
                shared.state().flow.lost();
                let detail = format!("Norn unreachable, holding readings: {why}");
                shared.set_reach(false, detail);
            }
            End::Draining => {
                let held = {
                    let mut state = shared.state();
                    state.flow.lost();
                    state.flow.held()
                };
                shared.tell(&Notice::Draining { held });
            }
        }
        if was_closing {
            break;
        }
        tokio::select! {
            () = sleep(backoff.next_wait()) => {}
            () = shared.closed() => {}
        }
    }
    let mut state = shared.state();
    let never_sent = state.flow.abandon();
    Report {
        counts: state.flow.counts(),
        never_sent,
    }
}

/// A channel to the daemon, kept alive.
pub(crate) async fn connect(shared: &Shared) -> Result<Channel, End> {
    let template = Endpoint::from_static("http://norn.invalid")
        .http2_keep_alive_interval(KEEP_ALIVE)
        .keep_alive_while_idle(true);
    norn_ipc::connect_with(&shared.endpoint, template)
        .await
        .map_err(lost)
}

/// Asks for what this sink cannot work without, and takes the limits.
pub(crate) async fn negotiate(shared: &Shared, channel: Channel) -> Result<(), End> {
    let mut required = vec![Capability::AppendSamples as i32];
    match shared.options.ack {
        AcknowledgementLevel::Durable => required.push(Capability::AckDurable as i32),
        AcknowledgementLevel::Persisted => required.push(Capability::AckPersisted as i32),
        _ => {}
    }
    let request = NegotiateProtocolRequest {
        name: "server".to_owned(),
        client_version: Some(admin::ProtocolVersion {
            major: PROTOCOL_VERSION.major,
            minor: PROTOCOL_VERSION.minor,
        }),
        required_capabilities: required,
        user_agent: shared.options.user_agent.clone(),
    };
    let reply = AdminServiceClient::new(channel)
        .negotiate_protocol(request)
        .await
        .map_err(|s| lost(s.message()))?
        .into_inner();
    let mut state = shared.state();
    // Asked anew on each connection: the daemon may have been replaced by one
    // that publishes differently, or nothing at all.
    state.published_limit = reply
        .limits
        .as_ref()
        .and_then(limits::published_message_limit);
    if let Some(l) = reply.limits {
        state.flow.limit(
            l.max_batch_samples,
            l.max_batch_bytes,
            l.max_pending_batches,
        );
    }
    Ok(())
}
