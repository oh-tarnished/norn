//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Granting rings: every bound on the shared-memory data plane is enforced here.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use norn_ipc::Endpoint;
use norn_protocol::admin::SharedMemoryPlane;
use norn_runtime::Budget;
use norn_shared_memory::{
    OWNER_ONLY_MAPPINGS, Ring, Settings, SharedMemoryTicket, create_root, ring_bytes, ring_prefix,
    ring_root,
};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_util::task::TaskTracker;

use super::pump::{self, Attached, Item};
use crate::metered::MeterLimits;

/// The daemon's offer. Concurrency: shared by every `AppendSamples` call.
#[derive(Debug)]
pub(crate) struct Plane {
    settings: Settings,
    /// `{settings.service}/{pid}`: a restarted daemon never meets a dead one's.
    service: String,
    root: PathBuf,
    prefix: String,
    rings: Arc<Semaphore>,
    next: AtomicU64,
    /// Where each ring's reader runs, so shutdown waits for it with the calls.
    calls: TaskTracker,
    pub(super) budget: Budget,
    pub(super) limits: MeterLimits,
}

fn unavailable(why: &str) -> Option<Arc<Plane>> {
    tracing::warn!(
        why,
        "shared memory is not offered; every call stays on gRPC"
    );
    None
}

impl Plane {
    /// The plane `settings` describe, or `None` when rings cannot be offered:
    /// off, not on a Unix socket (only its 0700 directory proves the client is
    /// this user, and hides the files), or on a platform whose mappings are not
    /// owner-only. Every `None` leaves clients on gRPC, which is always correct.
    pub(crate) fn new(
        settings: &Settings,
        endpoint: &Endpoint,
        budget: Budget,
        limits: MeterLimits,
        calls: &TaskTracker,
    ) -> Option<Arc<Self>> {
        if !settings.enabled {
            return None;
        }
        let Endpoint::Unix(socket) = endpoint else {
            return unavailable("it needs a Unix socket endpoint");
        };
        let Some(root) = ring_root(socket).filter(|_| OWNER_ONLY_MAPPINGS) else {
            return unavailable("this platform's mappings are not owner-only");
        };
        if let Err(e) = create_root(&root) {
            tracing::warn!(error = %e, "cannot create the ring directory");
            return unavailable("no ring directory");
        }
        Some(Arc::new(Self {
            settings: settings.clone(),
            service: format!("{}/{}", settings.service, std::process::id()),
            prefix: ring_prefix(&root),
            root,
            rings: Arc::new(Semaphore::new(settings.max_rings)),
            next: AtomicU64::new(0),
            calls: calls.clone(),
            budget,
            limits,
        }))
    }

    /// What `NegotiateProtocol` tells a client.
    pub(crate) fn advertised(&self) -> SharedMemoryPlane {
        let s = &self.settings;
        let narrow = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        let wide = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        SharedMemoryPlane {
            service: self.service.clone(),
            root_directory: self.root.to_string_lossy().into_owned(),
            name_prefix: self.prefix.clone(),
            max_frame_bytes: wide(s.max_frame_bytes as u64),
            slot_count: narrow(s.slot_count),
            max_rings: narrow(s.max_rings),
            segment_bytes: wide(ring_bytes(s)),
        }
    }

    /// Frames a ring holds.
    pub(super) fn slots(&self) -> usize {
        self.settings.slot_count
    }

    /// A ring for one call, or `None` to stay on gRPC: out of rings, out of
    /// budget, or iceoryx2 refused. The reader holds the ring, its slot and its
    /// charge, and gives them all back when it ends.
    pub(crate) async fn attach(self: &Arc<Self>) -> Option<(String, Attached)> {
        let slot = Arc::clone(&self.rings).try_acquire_owned().ok()?;
        let charge = self.budget.try_reserve(ring_bytes(&self.settings)).ok()?;
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let ticket = SharedMemoryTicket {
            service: format!("{}/{n}", self.service),
            root_directory: self.root.clone(),
            name_prefix: self.prefix.clone(),
            slot_count: self.settings.slot_count,
            max_frame_bytes: self.settings.max_frame_bytes,
        };
        let (items, frames) = mpsc::channel::<Item>(2);
        let (ready, created) = oneshot::channel();
        let (plane, control) = (Arc::clone(self), pump::Control::new());
        let (for_reader, wanted) = (Arc::clone(&control), ticket.clone());
        let handle = tokio::runtime::Handle::current();
        self.calls.spawn_blocking(move || {
            let ring = match Ring::create(&wanted) {
                Ok(ring) => ring,
                Err(e) => {
                    let _ = ready.send(Err(e));
                    return;
                }
            };
            let _ = ready.send(Ok(()));
            let _held = (slot, charge);
            pump::run(&ring, &plane, &handle, &items, &for_reader);
            ring.reap();
            drop(ring);
            let _ = items.blocking_send(Item::Done);
        });
        match created.await {
            Ok(Ok(())) => Some((ticket.service, Attached { frames, control })),
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "no ring; the call stays on gRPC");
                None
            }
            Err(_) => {
                tracing::warn!("iceoryx2 panicked creating a ring; the call stays on gRPC");
                None
            }
        }
    }
}
