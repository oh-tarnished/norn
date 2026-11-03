//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's gRPC control plane over local IPC (SPEC §6–7).
//!
//! **Owns:** running a tonic [`Router`](tonic::transport::server::Router)
//! on a local [`Endpoint`] ([`serve`], [`Server`]) and opening tonic
//! [`Channel`](tonic::transport::Channel)s to one ([`connect`]). The
//! transport — Unix socket, named pipe, loopback TCP — is chosen by the
//! endpoint alone; neither services nor clients see it.
//!
//! **Does not:** define services or messages (the protocol crate does),
//! implement transports (`norn-transport` does), or carry bulk data: the
//! [`DataPlane`] trait is the documented seam for that.
//!
//! **Lifecycle:** [`Server::bind`] claims the endpoint and spawns nothing;
//! [`Server::serve`] runs until its `CancellationToken` is cancelled, drains
//! in-flight calls up to a bound, aborts the rest, and returns only after
//! every connection task has stopped.
//!
//! **Unsafe:** none.
//!
//! ```no_run
//! use norn_ipc::{Endpoint, Server, ServerConfig, connect};
//! use tokio_util::sync::CancellationToken;
//!
//! # async fn demo(router: tonic::transport::server::Router) -> Result<(), norn_ipc::IpcError> {
//! let endpoint = Endpoint::platform_default();
//! let server = Server::bind(&endpoint, ServerConfig::default()).await?;
//! let shutdown = CancellationToken::new();
//! let running = tokio::spawn(server.serve(router, shutdown.clone()));
//!
//! let channel = connect(&endpoint).await?; // wrap in any generated client
//! drop(channel);
//!
//! shutdown.cancel();
//! let report = running.await.ok();
//! # drop(report);
//! # Ok(()) }
//! ```

mod client;
mod data_plane;
mod error;
mod server;
mod stream;

pub use client::{connect, connect_lazy, connect_with};
pub use data_plane::{ControlPlaneOnly, DataPlane, DataPlaneKind, SendFuture, negotiate};
pub use error::IpcError;
pub use norn_transport::{Endpoint, Limits, Overflow};
pub use server::{Server, ServerConfig, ShutdownReport, serve};
pub use stream::IpcStream;
