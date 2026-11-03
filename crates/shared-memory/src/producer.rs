//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The client's end of a ring.

use std::io;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use iceoryx2::port::publisher::Publisher;
use iceoryx2::port::{BackpressureAction, LoanError, SendError};
use iceoryx2::prelude::*;
use iceoryx2::service::port_factory::publish_subscribe::PortFactory as Ports;
use norn_ipc::{DataPlane, DataPlaneKind, SendFuture, SharedMemoryTicket};

use crate::error::{PlaneError, setup};
use crate::node::{Flavour, LOANS, name, node};

/// How long a full ring is waited on between attempts.
pub(crate) const FULL_WAIT: Duration = Duration::from_micros(50);

/// The publisher of one ring.
///
/// A frame is copied in and only then published, so a producer dying mid-write
/// publishes nothing: the daemon sees whole frames or none (SPEC §20). A full
/// ring refuses the frame rather than dropping it: [`try_send`](Self::try_send)
/// fails with `WouldBlock`, and [`DataPlane::send`] waits.
///
/// Concurrency: `Send + Sync`; a mutex serialises sends.
#[derive(Debug)]
pub struct Producer {
    publisher: Mutex<Publisher<Flavour, [u8], ()>>,
    service: Ports<Flavour, [u8], ()>,
    _node: Node<Flavour>,
    limit: usize,
}

impl Producer {
    /// Opens the service the daemon created for `ticket`, as its only publisher.
    ///
    /// # Errors
    ///
    /// [`PlaneError::Setup`] when it cannot be reached (another user, another
    /// container, a daemon gone, a publisher already attached): the caller
    /// stays on gRPC.
    pub fn attach(ticket: &SharedMemoryTicket) -> Result<Self, PlaneError> {
        let node = node(ticket)?;
        let service = node
            .service_builder(&name(ticket)?)
            .publish_subscribe::<[u8]>()
            .open()
            .map_err(setup("open the service"))?;
        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(ticket.max_frame_bytes)
            .max_loaned_samples(LOANS)
            // Without this a full ring would spin inside `send`, unbounded.
            .set_backpressure_handler(|_: &_| BackpressureAction::DiscardDataAndFail)
            .create()
            .map_err(setup("create the publisher"))?;
        Ok(Self {
            publisher: Mutex::new(publisher),
            service,
            _node: node,
            limit: ticket.max_frame_bytes,
        })
    }

    /// Publishes `frame` once, copying it. Borrows `frame` only for the copy.
    ///
    /// # Errors
    ///
    /// `WouldBlock` when the ring is full: nothing was delivered. `InvalidInput`
    /// above the frame limit. `BrokenPipe` when no daemon is attached. iceoryx2
    /// reports both of those last two outcomes as `Ok(0)`, a successful send to
    /// nobody, which is exactly how data would be lost silently, so it is read
    /// here: nothing is treated as sent unless a subscriber took it. A daemon
    /// that was killed still counts as attached, so a dead one looks like a full
    /// ring; `norn_ipc::AppendCall` learns of it from the gRPC call.
    pub fn try_send(&self, frame: &[u8]) -> io::Result<()> {
        if frame.len() > self.limit {
            let text = "the frame exceeds the ring's max_frame_bytes";
            return Err(io::Error::new(io::ErrorKind::InvalidInput, text));
        }
        let publisher = self
            .publisher
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let sample = match publisher.loan_slice_uninit(frame.len()) {
            Ok(sample) => sample,
            Err(LoanError::OutOfMemory | LoanError::ExceedsMaxLoans) => {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            Err(e) => return Err(io::Error::other(format!("{e:?}"))),
        };
        match sample.write_from_slice(frame).send() {
            Ok(0) if self.service.dynamic_config().number_of_subscribers() > 0 => {
                Err(io::ErrorKind::WouldBlock.into())
            }
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "no daemon is attached to the ring",
            )),
            Ok(_) => Ok(()),
            Err(SendError::UnableToDeliver) => Err(io::ErrorKind::WouldBlock.into()),
            Err(e) => Err(io::Error::other(format!("{e:?}"))),
        }
    }
}

impl DataPlane for Producer {
    fn kind(&self) -> DataPlaneKind {
        DataPlaneKind::SharedMemory
    }

    fn max_frame(&self) -> usize {
        self.limit
    }

    /// Waits while the ring is full, 50 microseconds at a time; cancel the future
    /// to stop waiting. Every other outcome of [`Producer::try_send`] returns.
    fn send<'a>(&'a self, frame: &'a [u8]) -> SendFuture<'a> {
        Box::pin(async move {
            loop {
                match self.try_send(frame) {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(FULL_WAIT).await;
                    }
                    done => return done,
                }
            }
        })
    }
}
