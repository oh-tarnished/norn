//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The client's way in: `norn_ipc::Attach` over [`Producer`].

use std::io;

use norn_ipc::{Attach, DataPlane, SharedMemoryTicket};

use crate::producer::Producer;

/// Attaches `norn_ipc::AppendCall`s to the rings a daemon grants. Pass it to
/// `norn_ipc::AppendOptions::shared_memory`; that is the client's whole opt-in.
///
/// `Default`, stateless. Concurrency: `Send + Sync`; one value serves any
/// number of calls, each with its own [`Producer`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SharedMemory;

impl Attach for SharedMemory {
    fn attach(&self, ticket: &SharedMemoryTicket) -> io::Result<Box<dyn DataPlane>> {
        Ok(Box::new(Producer::attach(ticket)?))
    }
}
