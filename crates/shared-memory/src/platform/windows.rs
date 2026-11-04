//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows: iceoryx2 0.10 calls `CreateFileMappingA` with no security attributes
//! (`iceoryx2-pal-posix`, `windows/mman.rs`), so a mapping gets the token's
//! default descriptor rather than the owner-only one SPEC §41 asks for, and the
//! API offers no way to pass one. The daemon therefore offers no rings here, and
//! clients stay on the named pipe; iceoryx2 is not even built, and [`Ring`] and
//! [`SharedMemory`] stand in for the Unix ones so their callers need no `cfg`.

use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};

use norn_ipc::{Attach, DataPlane, SharedMemoryTicket};

use crate::error::PlaneError;

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

/// Stands in for the daemon's end of a ring, which Windows never has: iceoryx2
/// is not built here. It cannot be made, so the daemon's ring path compiles
/// unchanged and is never reached ([`ring_root`] gives no directory).
///
/// Concurrency: uninhabited.
#[derive(Debug)]
pub struct Ring(Infallible);

impl Ring {
    /// Always refuses: no ring is offered on Windows.
    ///
    /// # Errors
    ///
    /// Always [`PlaneError::Setup`].
    pub fn create(_ticket: &SharedMemoryTicket) -> Result<Self, PlaneError> {
        Err(PlaneError::Setup {
            step: "create a ring",
            detail: "Windows offers no shared-memory rings".to_owned(),
        })
    }

    /// Never called: no `Ring` exists.
    ///
    /// # Errors
    ///
    /// None; uninhabited.
    pub fn next<T>(&self, _visit: impl FnOnce(&[u8]) -> T) -> Result<Option<T>, PlaneError> {
        match self.0 {}
    }

    /// Never called: no `Ring` exists.
    pub fn reap(&self) {
        match self.0 {}
    }
}

/// The client's opt-in, on Windows: every attach is refused, so a call stays on
/// the named pipe. A Windows daemon never grants a ring, so it is never asked.
///
/// `Default`, stateless. Concurrency: `Send + Sync`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SharedMemory;

impl Attach for SharedMemory {
    fn attach(&self, _ticket: &SharedMemoryTicket) -> io::Result<Box<dyn DataPlane>> {
        Err(io::ErrorKind::Unsupported.into())
    }
}
