//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The one place iceoryx2 is configured, so both ends agree. Its sizes are
//! [`crate::sizing`]'s, which every target has.

use std::sync::Once;

use iceoryx2::config::Config;
use iceoryx2::prelude::*;
use norn_ipc::SharedMemoryTicket;

pub(crate) use crate::sizing::{BORROWED, LOANS};

use crate::error::PlaneError;

/// The iceoryx2 flavour: `Send + Sync` ports over inter-process memory.
pub(crate) type Flavour = ipc_threadsafe::Service;

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

/// Wraps an iceoryx2 error, which is `Debug` and has no stable text.
pub(super) fn setup<E: std::fmt::Debug>(step: &'static str) -> impl FnOnce(E) -> PlaneError {
    move |e| PlaneError::Setup {
        step,
        detail: format!("{e:?}"),
    }
}
