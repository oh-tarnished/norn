//! Streamable HTTP at `/mcp`, with no web framework: one route.
//!
//! A local server's danger is a web page in the user's browser making requests to it.
//! DNS rebinding gives such a page a loopback address under its own name, and only
//! `Host` tells them apart; so a name that is not loopback's or listed with
//! `--mcp-host` gets 403 before anything runs, as does a browser `Origin` from
//! elsewhere. rmcp enforces both, from the lists given here.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tower::{Service, service_fn};

use crate::server::NornMcp;

/// The most connections served at once; another waits in the accept queue. It holds
/// each connection's buffers to a bound, and is more than a loopback client wants.
const MAX_CONNECTIONS: usize = 64;

const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

fn config(hosts: &[String], cancel: CancellationToken) -> StreamableHttpServerConfig {
    let names = || {
        LOOPBACK
            .iter()
            .map(|h| (*h).to_owned())
            .chain(hosts.iter().cloned())
    };
    let origins = names().flat_map(|name| {
        let host = if name.contains(':') {
            format!("[{name}]")
        } else {
            name
        };
        [format!("http://{host}:*"), format!("https://{host}:*")]
    });
    StreamableHttpServerConfig::default()
        .with_allowed_hosts(names())
        .with_allowed_origins(origins)
        .with_sse_retry(None)
        .with_cancellation_token(cancel)
}

fn not_found() -> Response<BoxBody<Bytes, Infallible>> {
    let mut response = Response::new(Empty::new().boxed());
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// Serves `server` on `listener` until `cancel`, returning only when every connection
/// task has stopped. Each client session gets a clone of `server`.
pub async fn serve(
    listener: TcpListener,
    server: NornMcp,
    hosts: &[String],
    cancel: CancellationToken,
) {
    let factory = move || Ok(server.clone());
    let sessions = Arc::new(LocalSessionManager::default());
    let mcp = StreamableHttpService::new(factory, sessions, config(hosts, cancel.child_token()));
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut tasks = JoinSet::new();
    loop {
        let permit = tokio::select! {
            () = cancel.cancelled() => break,
            permit = Arc::clone(&slots).acquire_owned() => permit,
        };
        let Ok(permit) = permit else { break };
        let accepted = tokio::select! {
            () = cancel.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let Ok((stream, _)) = accepted else {
            // Out of descriptors, usually: wait rather than spin.
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let mcp = mcp.clone();
        let route = service_fn(move |request: Request<hyper::body::Incoming>| {
            let mut mcp = mcp.clone();
            async move {
                if request.uri().path() == "/mcp" {
                    mcp.call(request).await
                } else {
                    Ok::<_, Infallible>(not_found())
                }
            }
        });
        tasks.spawn(async move {
            let _permit = permit;
            let service = TowerToHyperService::new(route);
            let builder = Builder::new(TokioExecutor::new());
            let _ = builder
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
        while tasks.try_join_next().is_some() {}
    }
    tasks.shutdown().await;
}
