//! The `norn-protocol-version` metadata check (docs/reference/protocol.md, "Versioning and
//! capability negotiation"), as a layer in front of every service.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use norn_protocol::{PROTOCOL_VERSION, VERSION_METADATA_KEY};
use tonic::body::Body;
use tower::{Layer, Service};

use crate::status::version_unsupported;

/// Exempt: docs/reference/protocol.md promises `GetServer` to any client, however old, so
/// it can learn what it is talking to.
const EXEMPT_PATH: &str = "/norn.admin.v1.AdminService/GetServer";

/// Adds [`VersionGate`] around a service. Stateless; `Copy`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VersionGateLayer;

impl<S> Layer<S> for VersionGateLayer {
    type Service = VersionGate<S>;

    fn layer(&self, inner: S) -> VersionGate<S> {
        VersionGate { inner }
    }
}

/// Fails a call whose `norn-protocol-version` names another major, or is not
/// `major.minor` at all, with `FAILED_PRECONDITION` and
/// `ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED`, before any handler runs. A
/// call without the header passes: sending it is optional.
///
/// Concurrency: `Clone`, and as concurrent as the service it wraps.
/// Performance: one header lookup and one boxed future per call.
#[derive(Debug, Clone)]
pub struct VersionGate<S> {
    inner: S,
}

/// Whether `text` is a `major.minor` this daemon can speak.
fn speaks(text: &str) -> bool {
    let Some((major, minor)) = text.split_once('.') else {
        return false;
    };
    minor.parse::<i32>().is_ok() && major.parse() == Ok(PROTOCOL_VERSION.major)
}

/// The refusal for `request`, if it gets one.
fn refusal<B>(request: &http::Request<B>) -> Option<tonic::Status> {
    if request.uri().path() == EXEMPT_PATH {
        return None;
    }
    let value = request.headers().get(VERSION_METADATA_KEY)?;
    let text = String::from_utf8_lossy(value.as_bytes());
    (!speaks(&text)).then(|| version_unsupported(&text))
}

impl<S, B> Service<http::Request<B>> for VersionGate<S>
where
    S: Service<http::Request<B>, Response = http::Response<Body>>,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        match refusal(&request) {
            Some(status) => Box::pin(std::future::ready(Ok(status.into_http()))),
            None => Box::pin(self.inner.call(request)),
        }
    }
}
