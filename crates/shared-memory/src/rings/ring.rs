//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The daemon's end of a ring.

use iceoryx2::port::subscriber::Subscriber;
use iceoryx2::prelude::*;
use iceoryx2::service::port_factory::publish_subscribe::PortFactory as Ports;
use norn_ipc::SharedMemoryTicket;

use super::node::{BORROWED, Flavour, name, node, setup};
use crate::error::PlaneError;

/// One bounded service with one subscriber, created by the daemon so the client
/// cannot choose its limits: one publisher, one subscriber, two nodes, no
/// history, and no overflow, so a full ring refuses rather than overwrites.
///
/// Concurrency: `Send + Sync`, but [`next`](Self::next) is meant for one thread.
/// Dropping it removes the service; a dead producer's leftovers are removed by
/// [`reap`](Self::reap).
#[derive(Debug)]
pub struct Ring {
    subscriber: Subscriber<Flavour, [u8], ()>,
    _service: Ports<Flavour, [u8], ()>,
    node: Node<Flavour>,
    limit: usize,
}

impl Ring {
    /// Creates the service `ticket` names. The directory must already exist and
    /// be owner-only: [`create_root`](crate::create_root) sees to that.
    ///
    /// # Errors
    ///
    /// [`PlaneError::Setup`] if iceoryx2 refuses; the call stays on gRPC.
    pub fn create(ticket: &SharedMemoryTicket) -> Result<Self, PlaneError> {
        let node = node(ticket)?;
        // Whatever an earlier crash left behind is reclaimed before more is made.
        let _ = node.try_cleanup_dead_nodes();
        let service = node
            .service_builder(&name(ticket)?)
            .publish_subscribe::<[u8]>()
            .max_publishers(1)
            .max_subscribers(1)
            .max_nodes(2)
            .history_size(0)
            .subscriber_max_buffer_size(ticket.slot_count)
            .subscriber_max_borrowed_samples(BORROWED)
            .enable_safe_overflow(false)
            .create()
            .map_err(setup("create the service"))?;
        let subscriber = service
            .subscriber_builder()
            .buffer_size(ticket.slot_count)
            .create()
            .map_err(setup("create the subscriber"))?;
        Ok(Self {
            subscriber,
            _service: service,
            node,
            limit: ticket.max_frame_bytes,
        })
    }

    /// Gives the next frame, if one is waiting, to `visit`, which must copy what
    /// it wants: the bytes are shared with a peer who can change them while they
    /// are read, so nothing may be checked here and trusted later. The borrow
    /// ends, and the frame's slot is released, when `visit` returns.
    ///
    /// # Errors
    ///
    /// [`PlaneError::FrameTooLarge`] when the frame claims more than the ring
    /// holds, before `visit` sees it. [`PlaneError::Receive`] if iceoryx2 fails.
    /// Either way the ring should be abandoned.
    pub fn next<T>(&self, visit: impl FnOnce(&[u8]) -> T) -> Result<Option<T>, PlaneError> {
        let sample = self
            .subscriber
            .receive()
            .map_err(|e| PlaneError::Receive(format!("{e:?}")))?;
        let Some(sample) = sample else {
            return Ok(None);
        };
        let frame = sample.payload();
        if frame.len() > self.limit {
            let (length, limit) = (frame.len(), self.limit);
            return Err(PlaneError::FrameTooLarge { length, limit });
        }
        Ok(Some(visit(frame)))
    }

    /// Reclaims what dead nodes left. Cheap enough to run when a call ends.
    pub fn reap(&self) {
        let _ = self.node.try_cleanup_dead_nodes();
    }
}
