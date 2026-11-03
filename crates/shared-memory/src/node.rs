//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The one place iceoryx2 is configured, so both ends agree.

use std::sync::Once;

use iceoryx2::config::Config;
use iceoryx2::prelude::*;
use norn_ipc::SharedMemoryTicket;

use crate::error::{PlaneError, setup};
use crate::settings::Settings;

/// The iceoryx2 flavour: `Send + Sync` ports over inter-process memory.
pub(crate) type Flavour = ipc_threadsafe::Service;

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

/// A node for `ticket`. Its signal handling is off, because the daemon owns its
/// signals, and iceoryx2's logger is silenced.
pub(crate) fn node(ticket: &SharedMemoryTicket) -> Result<Node<Flavour>, PlaneError> {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| set_log_level(LogLevel::Fatal));
    let mut config = Config::default();
    let root = ticket.root_directory.to_string_lossy();
    let root = Path::new(root.as_bytes()).map_err(setup("use the ring directory"))?;
    config.global.set_root_path(&root);
    config.global.prefix =
        FileName::new(ticket.name_prefix.as_bytes()).map_err(setup("use the ring prefix"))?;
    NodeBuilder::new()
        .config(&config)
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<Flavour>()
        .map_err(setup("create an iceoryx2 node"))
}

/// The service's name.
pub(crate) fn name(ticket: &SharedMemoryTicket) -> Result<ServiceName, PlaneError> {
    ServiceName::new(&ticket.service).map_err(setup("name the service"))
}
