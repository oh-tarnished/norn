//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a client needs to attach to one shared-memory ring, and the seam through
//! which a data-plane crate attaches it.

use std::io;
use std::path::PathBuf;

use norn_protocol::admin::SharedMemoryPlane;

use crate::DataPlane;

/// Everything needed to reach one ring: the daemon's plane, as
/// `NegotiateProtocol` described it, and the service the daemon granted one
/// `AppendSamples` call. The daemon builds the same value to create the ring, so
/// both ends agree on every limit; the client cannot choose one.
///
/// Concurrency: plain owned data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedMemoryTicket {
    /// The ring's whole iceoryx2 service name.
    pub service: String,
    /// The owner-only directory iceoryx2 keeps its files in.
    pub root_directory: PathBuf,
    /// The prefix of every name iceoryx2 creates for this daemon.
    pub name_prefix: String,
    /// Frames the ring holds.
    pub slot_count: usize,
    /// The largest frame, in bytes.
    pub max_frame_bytes: usize,
}

impl SharedMemoryTicket {
    /// The ticket for the ring `granted` (the response metadata
    /// `norn-shared-memory-service`) under `plane`. Copies out of both.
    ///
    /// `None` when `granted` is not under `plane.service`, or a limit is not a
    /// positive number this platform can hold: the client then stays on gRPC,
    /// which is always correct, rather than attaching to something it was not
    /// offered.
    #[must_use]
    pub fn granted(plane: &SharedMemoryPlane, granted: &str) -> Option<Self> {
        let suffix = granted.strip_prefix(plane.service.as_str())?;
        let ring = suffix.strip_prefix('/')?;
        if plane.service.is_empty() || ring.is_empty() {
            return None;
        }
        let slot_count = usize::try_from(plane.slot_count).ok().filter(|n| *n > 0)?;
        let max_frame_bytes = usize::try_from(plane.max_frame_bytes)
            .ok()
            .filter(|n| *n > 0)?;
        Some(Self {
            service: granted.to_owned(),
            root_directory: PathBuf::from(&plane.root_directory),
            name_prefix: plane.name_prefix.clone(),
            slot_count,
            max_frame_bytes,
        })
    }
}

/// Attaches a client to a granted ring: implemented by a data-plane crate
/// (`norn-shared-memory`'s `SharedMemory`), declared here so that this crate,
/// beneath it, can fall back to gRPC without knowing how a ring is reached.
///
/// Concurrency: `Send + Sync`; one value may attach many calls.
pub trait Attach: Send + Sync {
    /// Opens the ring `ticket` names.
    ///
    /// # Errors
    ///
    /// Any failure to reach it (another user, a container boundary, a daemon
    /// gone). The call then carries its batches over gRPC.
    fn attach(&self, ticket: &SharedMemoryTicket) -> io::Result<Box<dyn DataPlane>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane() -> SharedMemoryPlane {
        SharedMemoryPlane {
            service: "norn/42".into(),
            root_directory: "/run/n/iox2/".into(),
            name_prefix: "norn0000abcd_".into(),
            max_frame_bytes: 1024,
            slot_count: 4,
            max_rings: 2,
            segment_bytes: 6 * 1024,
        }
    }

    #[test]
    fn a_ring_under_the_offered_service_is_attached_with_the_offered_limits() {
        let ticket = SharedMemoryTicket::granted(&plane(), "norn/42/7").unwrap();
        assert_eq!(ticket.service, "norn/42/7");
        assert_eq!((ticket.slot_count, ticket.max_frame_bytes), (4, 1024));
    }

    #[test]
    fn a_ring_outside_the_offered_service_is_refused() {
        assert!(SharedMemoryTicket::granted(&plane(), "other/42/7").is_none());
        assert!(SharedMemoryTicket::granted(&plane(), "norn/421").is_none());
        assert!(SharedMemoryTicket::granted(&plane(), "norn/42/").is_none());
    }

    #[test]
    fn limits_that_are_not_positive_are_refused() {
        let mut bad = plane();
        bad.slot_count = 0;
        assert!(SharedMemoryTicket::granted(&bad, "norn/42/7").is_none());
        let mut bad = plane();
        bad.max_frame_bytes = -1;
        assert!(SharedMemoryTicket::granted(&bad, "norn/42/7").is_none());
    }
}
