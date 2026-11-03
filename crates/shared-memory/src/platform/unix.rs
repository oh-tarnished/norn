//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Linux and macOS: iceoryx2 creates shared memory `0600`, from its own constant
//! (`FINAL_PERMISSIONS`), so long as its `dev_permissions` feature stays off.

use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// Whether every mapping iceoryx2 creates here is reachable only by its owner.
pub const OWNER_ONLY_MAPPINGS: bool = true;

/// Where iceoryx2 keeps its files: beside the socket, in the directory
/// `norn-transport` has already checked is `0700`, not in the world-listable
/// `/tmp/iceoryx2`. `None` for a socket with no parent.
#[must_use]
pub fn ring_root(socket: &Path) -> Option<PathBuf> {
    Some(socket.parent()?.join("iox2/"))
}

/// Makes `root` with mode `0700`.
///
/// # Errors
///
/// When it cannot be created.
pub fn create_root(root: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)
}
