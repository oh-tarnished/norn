//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows: iceoryx2 0.10 calls `CreateFileMappingA` with no security attributes
//! (`iceoryx2-pal-posix`, `windows/mman.rs`), so a mapping gets the token's
//! default descriptor rather than the owner-only one SPEC §41 asks for, and the
//! API offers no way to pass one. The daemon therefore offers no rings here, and
//! clients stay on the named pipe.

use std::io;
use std::path::{Path, PathBuf};

/// Whether every mapping iceoryx2 creates here is reachable only by its owner.
pub const OWNER_ONLY_MAPPINGS: bool = false;

/// No directory protects a Windows mapping, so there is none to give.
#[must_use]
pub fn ring_root(_socket: &Path) -> Option<PathBuf> {
    None
}

/// Never reached: [`ring_root`] gives no directory.
///
/// # Errors
///
/// Always `Unsupported`.
pub fn create_root(_root: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}
