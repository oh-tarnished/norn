//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! tonic client channels over any local endpoint.

use hyper_util::rt::TokioIo;
use norn_transport::{Endpoint, connect as dial};
use tonic::transport::{Channel, Endpoint as ChannelTemplate, Uri};

use crate::IpcError;

/// The URI tonic is given. Its authority becomes the HTTP/2 `:authority`
/// header and is otherwise unused: the connector, not DNS, decides where
/// bytes go. `.invalid` (RFC 2606) guarantees it never resolves.
const AUTHORITY: &str = "http://norn.invalid";

/// Connects to the daemon at `endpoint`, returning a tonic [`Channel`].
///
/// Works identically for `unix:`, `pipe:` and `tcp:`; generated gRPC
/// clients wrap the channel without knowing the transport. Establishes the
/// first connection eagerly, so a missing daemon fails here.
///
/// The channel is `Clone + Send + Sync`; clones share one HTTP/2
/// connection, which multiplexes concurrent calls. If the connection drops,
/// tonic reconnects through the same endpoint on the next call.
pub async fn connect(endpoint: &Endpoint) -> Result<Channel, IpcError> {
    connect_with(endpoint, ChannelTemplate::from_static(AUTHORITY)).await
}

/// As [`connect`], but with a caller-built tonic endpoint carrying timeouts,
/// keep-alive, window sizes and so on. Its URI is ignored for routing.
pub async fn connect_with(
    endpoint: &Endpoint,
    template: ChannelTemplate,
) -> Result<Channel, IpcError> {
    let target = endpoint.clone();
    let connector = tower::service_fn(move |_: Uri| {
        let target = target.clone();
        async move { dial(&target).await.map(TokioIo::new) }
    });
    Ok(template.connect_with_connector(connector).await?)
}

/// As [`connect`], but connects on first use rather than now. Never fails;
/// an unreachable daemon surfaces as `UNAVAILABLE` on the first call.
pub fn connect_lazy(endpoint: &Endpoint) -> Channel {
    let target = endpoint.clone();
    let connector = tower::service_fn(move |_: Uri| {
        let target = target.clone();
        async move { dial(&target).await.map(TokioIo::new) }
    });
    ChannelTemplate::from_static(AUTHORITY).connect_with_connector_lazy(connector)
}
