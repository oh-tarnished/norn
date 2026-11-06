//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Creating what readings need, in batches where the daemon has them
//! (`batch`), one at a time where it does not (`upsert`). `ALREADY_EXISTS`
//! means a past run did, so create is safe at every start (protocol.md,
//! "Writing a client" step 2); a resource that has since changed is then
//! updated. A signal's value type is immutable and never in an update. What
//! waits, and in which order, is `work`'s; carrying out one chunk, `chunk`'s.

use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use tonic::transport::Channel;
use tonic::{Code, Status};

mod batch;
mod chunk;
mod upsert;
mod work;

pub(crate) use chunk::{chunk, chunk_size};
pub(crate) use work::{Kind, Work};

/// Calls at once on each connection: Norn's limit.
pub(crate) const PER_CONNECTION: usize = 16;

/// Something Norn should know of.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Resource {
    /// An entity, by its full name.
    Entity(norn_protocol::entity::Entity),
    /// A signal.
    Signal {
        /// `entities/{entity}`.
        parent: String,
        /// The signal's ID.
        id: String,
        /// The signal, its full name in it.
        signal: norn_protocol::signal::Signal,
    },
}

/// Why a resource did not land.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The daemon went away: everything waits for it.
    Lost(String),
    /// It answered no, as past a limit: that one resource is given up, and its
    /// samples will be refused and counted.
    Refused(String),
}

/// Whether `status` means the daemon is away rather than that it said no.
pub(crate) fn failure(status: &Status) -> Failure {
    match status.code() {
        Code::Unavailable | Code::Cancelled | Code::DeadlineExceeded | Code::Unknown => {
            Failure::Lost(status.message().to_owned())
        }
        _ => Failure::Refused(status.to_string()),
    }
}

/// `result`, with `ALREADY_EXISTS` as `false`.
fn created<T>(result: Result<T, Status>) -> Result<bool, Failure> {
    match result {
        Ok(_) => Ok(true),
        Err(s) if s.code() == Code::AlreadyExists => Ok(false),
        Err(s) => Err(failure(&s)),
    }
}

/// Creates the stream `stream` (its ID), if it does not exist. An existing
/// stream's retention and annotations are the operator's, so left alone.
pub(crate) async fn stream(channel: Channel, stream: &str) -> Result<(), Failure> {
    let request = CreateStreamRequest {
        stream_id: stream.to_owned(),
        stream: Some(Stream::default()),
        validate_only: false,
    };
    let mut client = StreamServiceClient::new(channel);
    created(client.create_stream(request).await).map(|_| ())
}

/// The ID a full name ends in.
pub(crate) fn last(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or_default()
}
