//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An admitted server-side connection: the raw stream plus its slot, its
//! idle timer and its abort signal.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::{Instant, Sleep};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

use crate::BoxIo;

/// A connection accepted by [`Incoming`](crate::Incoming).
///
/// Reads and writes pass straight through to the transport, with two
/// additions, both reported as I/O errors so any protocol stack on top
/// (hyper, here) tears the connection down through its normal error path:
///
/// - **Idle timeout**: if no byte moves in either direction for the
///   configured duration, the pending read or write fails with
///   [`io::ErrorKind::TimedOut`].
/// - **Abort**: once the server's abort token is cancelled (a bounded drain
///   expired), every operation fails with [`io::ErrorKind::ConnectionAborted`].
///
/// Dropping a `Connection` closes it and frees its slot, letting the next
/// queued client in.
///
/// # Concurrency
///
/// `Send` and `Unpin`; owned by exactly one task, as `AsyncRead`/`AsyncWrite`
/// demand. Not `Sync`, and there is no reason to share it.
pub struct Connection {
    io: BoxIo,
    idle: Option<(Duration, Pin<Box<Sleep>>)>,
    abort: Pin<Box<WaitForCancellationFutureOwned>>,
    _slot: OwnedSemaphorePermit,
}

impl Connection {
    pub(crate) fn new(
        io: BoxIo,
        slot: OwnedSemaphorePermit,
        idle: Option<Duration>,
        abort: CancellationToken,
    ) -> Self {
        let idle = idle.map(|d| (d, Box::pin(tokio::time::sleep(d))));
        Self {
            io,
            idle,
            abort: Box::pin(abort.cancelled_owned()),
            _slot: slot,
        }
    }

    /// Fails if the server has aborted connections; registers for wake-up
    /// otherwise, so a connection parked in `Pending` still notices.
    fn check_abort(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        match self.abort.as_mut().poll(cx) {
            Poll::Ready(()) => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "server shutdown drain expired",
            )),
            Poll::Pending => Ok(()),
        }
    }

    /// Pushes the idle deadline out after traffic.
    fn touch(&mut self) {
        if let Some((d, sleep)) = self.idle.as_mut() {
            sleep.as_mut().reset(Instant::now() + *d);
        }
    }

    /// Converts a `Pending` transport into a timeout once idle too long.
    fn idle_expired(&mut self, cx: &mut Context<'_>) -> bool {
        match self.idle.as_mut() {
            Some((_, sleep)) => sleep.as_mut().poll(cx).is_ready(),
            None => false,
        }
    }
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "connection idle timeout")
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.check_abort(cx)?;
        let before = buf.filled().len();
        match Pin::new(&mut this.io).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if buf.filled().len() > before {
                    this.touch();
                }
                Poll::Ready(Ok(()))
            }
            Poll::Pending if this.idle_expired(cx) => Poll::Ready(Err(timed_out())),
            other => other,
        }
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.check_abort(cx)?;
        match Pin::new(&mut this.io).poll_write(cx, data) {
            Poll::Ready(Ok(n)) => {
                if n > 0 {
                    this.touch();
                }
                Poll::Ready(Ok(n))
            }
            Poll::Pending if this.idle_expired(cx) => Poll::Ready(Err(timed_out())),
            other => other,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.check_abort(cx)?;
        Pin::new(&mut this.io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Shutdown is allowed after abort: it only closes our side.
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}
