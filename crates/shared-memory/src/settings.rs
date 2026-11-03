//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a daemon offers: the `[shared_memory]` table.

/// The daemon's data-plane configuration.
///
/// `Default` is **off**: gRPC on the local socket meets the loads Norn targets,
/// and a ring widens what a misbehaving process of the same user can do to the
/// daemon (docs/reference/shared-memory.md). When on: service `norn`, two rings,
/// four slots of 1 MiB each. Plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Whether the daemon offers rings at all.
    pub enabled: bool,
    /// The iceoryx2 service name rings are named under; the daemon appends its
    /// process id, so a restarted daemon never meets a dead one's rings.
    pub service: String,
    /// Rings open at once; a call past it stays on gRPC.
    pub max_rings: usize,
    /// Frames a ring holds before the producer waits.
    pub slot_count: usize,
    /// The largest frame. A larger batch travels by gRPC, in order.
    pub max_frame_bytes: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            service: "norn".to_owned(),
            max_rings: 2,
            slot_count: 4,
            max_frame_bytes: 1024 * 1024,
        }
    }
}
