//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Platform selection (SPEC §5): Unix has iceoryx2's rings, Windows stand-ins
//! that refuse. The only other `cfg` builds `rings`, the iceoryx2 half, on Unix.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub use os::{OWNER_ONLY_MAPPINGS, Ring, SharedMemory, create_root, ring_root};
