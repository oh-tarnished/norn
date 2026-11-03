//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Every service the schema defines but this stage does not implement.
//!
//! Each is registered under its real name, so a client calling it gets a
//! typed `UNIMPLEMENTED` with an `ErrorInfo` in the `norn.local` domain
//! rather than tonic's bare one. Reason: `ERROR_REASON_NOT_IMPLEMENTED`, with
//! metadata `service`, `method`, and `capability` where one gates it.

use std::convert::Infallible;
use std::future::{Ready, ready};
use std::marker::PhantomData;
use std::task::{Context, Poll};

use norn_protocol::common::ErrorReason;
use tonic::Code;
use tonic::body::Body;
use tonic::server::NamedService;

use crate::status::error_info;

/// The `UNIMPLEMENTED` status for one method, for a service that has others.
pub(crate) fn not_implemented(
    service: &str,
    method: &str,
    capability: Option<&str>,
) -> tonic::Status {
    let mut metadata = vec![
        ("service", service.to_owned()),
        ("method", method.to_owned()),
    ];
    if let Some(capability) = capability {
        metadata.push(("capability", capability.to_owned()));
    }
    error_info(
        Code::Unimplemented,
        ErrorReason::NotImplemented,
        format!("{service}/{method} is not implemented by this daemon yet"),
        metadata,
    )
}

/// A service name, and the capability that will announce it.
pub trait Stubbed: Send + Sync + 'static {
    /// The fully qualified gRPC service name.
    const NAME: &'static str;
    /// The `Capability` name that will be offered once it exists, if any.
    const CAPABILITY: Option<&'static str>;
}

/// A tower service answering every method of `S` with `UNIMPLEMENTED`.
///
/// Concurrency: stateless, `Clone + Send + Sync`. Never reads the request
/// body: a trailers-only response ends the call at once.
pub struct Unimplemented<S>(PhantomData<fn() -> S>);

impl<S> Unimplemented<S> {
    /// The stub for `S`.
    #[must_use]
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<S> Default for Unimplemented<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Clone for Unimplemented<S> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<S: Stubbed> NamedService for Unimplemented<S> {
    const NAME: &'static str = S::NAME;
}

impl<S: Stubbed> tower::Service<http::Request<Body>> for Unimplemented<S> {
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<Body>) -> Self::Future {
        let method = req.uri().path().rsplit('/').next().unwrap_or_default();
        ready(Ok(
            not_implemented(S::NAME, method, S::CAPABILITY).into_http()
        ))
    }
}
