//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which plane a session's batches travel: the socket, or, with the
//! `shared-memory` feature and [`Options::shared_memory`](crate::Options::shared_memory),
//! a ring the daemon grants (protocol 1.7), through `norn_ipc::AppendCall`.
//!
//! Either way the session sees the same two ends: a sender, and replies over
//! gRPC, where every admission and acknowledgement arrives on both planes, so
//! `flow` decides as before. A daemon that is older, has shared memory off or
//! is out of rings leaves the call on the socket, transparently; so does a
//! batch too large for a ring frame, which `AppendCall` sends over gRPC and
//! stays there.

use crate::shared::Shared;
use norn_ipc::{AppendCall, AppendOptions};
use tonic::Status;
use tonic::transport::Channel;

/// Requests queued for the socket on one call. Few, as more only queues in the
/// client what the flow can hold and count.
pub(super) const QUEUED: usize = 4;

/// Whether this build can append through shared memory at all.
pub(super) const AVAILABLE: bool = cfg!(feature = "shared-memory");

/// What to open a call with.
fn options(shared: &Shared) -> AppendOptions {
    let options = AppendOptions::default().capacity(QUEUED);
    #[cfg(feature = "shared-memory")]
    if shared.options.shared_memory {
        return options.shared_memory(std::sync::Arc::new(norn_shared_memory::SharedMemory));
    }
    let _ = shared;
    options
}

/// Opens the one `AppendSamples` call, and says which plane it got if a ring
/// was asked for.
pub(super) async fn open(shared: &Shared, channel: Channel) -> Result<AppendCall, Status> {
    let call = AppendCall::open(channel, &options(shared)).await?;
    if AVAILABLE && shared.options.shared_memory {
        let on_ring = call.plane() != norn_ipc::DataPlaneKind::ControlPlane;
        shared.tell(&crate::health::Notice::Plane {
            shared_memory: on_ring,
        });
    }
    Ok(call)
}
