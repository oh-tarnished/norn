//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a ring costs and what its names start with: plain arithmetic, on every
//! target, so the daemon's configuration validates the same everywhere, even
//! where no ring is ever offered.

use crate::settings::Settings;

/// Frames the daemon may borrow at once: it copies one and returns it.
pub(crate) const BORROWED: usize = 1;
/// Frames a producer may hold while writing.
pub(crate) const LOANS: usize = 1;

/// Chunks iceoryx2 allocates for a ring of `slots` (its
/// `required_amount_of_samples_per_data_segment`, with one subscriber).
#[must_use]
pub fn chunks_per_ring(slots: usize) -> usize {
    slots.saturating_add(BORROWED).saturating_add(LOANS)
}

/// What one ring maps, charged to `ipc_bytes` while it is open: the segment
/// size `NegotiateProtocol` reports as `segment_bytes`.
#[must_use]
pub fn ring_bytes(settings: &Settings) -> u64 {
    let chunks = chunks_per_ring(settings.slot_count) as u64;
    chunks.saturating_mul(settings.max_frame_bytes as u64)
}

/// The prefix of everything iceoryx2 creates for rings under `root`: a short,
/// stable digest (FNV-1a), so one daemon's names stay apart from another's and
/// a restart finds what a crash left.
#[must_use]
pub fn ring_prefix(root: &std::path::Path) -> String {
    let digest = root
        .to_string_lossy()
        .bytes()
        .fold(0x811c_9dc5, |h: u32, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
    format!("norn{digest:08x}_")
}
