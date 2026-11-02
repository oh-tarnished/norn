//! The seam where a bulk data plane will plug in (SPEC §7, stage 12).
//!
//! Today every byte, control or data, travels as gRPC messages over the
//! control-plane connection: that is the "regular IPC" fallback SPEC §7
//! requires, and the only plane implemented. A later `shared-memory` crate
//! will implement [`DataPlane`] with a memory-mapped ring buffer; the
//! client and daemon will agree on it through capability negotiation over
//! the control plane, and fall back to [`ControlPlaneOnly`] whenever it is
//! unavailable (a different user, a container boundary, an OS without it).
//!
//! Framing for that plane is deliberately not defined here. See docs/reference/ipc.md
//! for the options evaluated and why they were deferred.

use std::future::Future;
use std::io;
use std::pin::Pin;

/// Which plane carries bulk sample batches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DataPlaneKind {
    /// Batches travel as gRPC messages on the control-plane connection.
    ControlPlane,
    /// Batches travel through a shared-memory ring buffer. Reserved: no
    /// implementation exists yet.
    SharedMemory,
}

/// The future returned by [`DataPlane::send`].
pub type SendFuture<'a> = Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>>;

/// A one-way channel for bulk frames from a client to the daemon.
///
/// # Contract for implementations
///
/// - **Bounded:** a full buffer makes `send` wait (backpressure) or fail
///   with `WouldBlock`; it never grows (SPEC §16).
/// - **Ordered and lossless:** frames arrive in send order or the plane
///   reports failure; nothing is silently dropped (SPEC §18).
/// - **Crash-aware:** a peer that dies mid-frame is detected and reported
///   by the receiver, never read as a valid frame (SPEC §20).
/// - **Concurrency:** `Send + Sync`; concurrent `send` calls are serialised
///   by the implementation, preserving per-call frame integrity.
pub trait DataPlane: Send + Sync + 'static {
    /// Which plane this is, for negotiation and metrics.
    fn kind(&self) -> DataPlaneKind;

    /// Largest frame `send` accepts, in bytes.
    fn max_frame(&self) -> usize;

    /// Sends one opaque frame. The bytes are copied before the future
    /// resolves; the caller may reuse its buffer afterwards.
    fn send<'a>(&'a self, frame: &'a [u8]) -> SendFuture<'a>;
}

/// The fallback: no separate data plane; bulk data goes over gRPC.
///
/// Its `send` always fails with [`io::ErrorKind::Unsupported`], telling the
/// caller to use the control-plane RPC instead. `Default`, `Copy`, stateless.
#[derive(Clone, Copy, Debug, Default)]
pub struct ControlPlaneOnly;

impl DataPlane for ControlPlaneOnly {
    fn kind(&self) -> DataPlaneKind {
        DataPlaneKind::ControlPlane
    }

    fn max_frame(&self) -> usize {
        0
    }

    fn send<'a>(&'a self, _frame: &'a [u8]) -> SendFuture<'a> {
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no data plane: send batches through the control-plane RPC",
            ))
        })
    }
}

/// Picks the best plane both sides support: the highest
/// [`DataPlaneKind`] present in both lists, or
/// [`DataPlaneKind::ControlPlane`], which every peer supports implicitly.
pub fn negotiate(ours: &[DataPlaneKind], theirs: &[DataPlaneKind]) -> DataPlaneKind {
    ours.iter()
        .filter(|k| theirs.contains(k))
        .max()
        .copied()
        .unwrap_or(DataPlaneKind::ControlPlane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_falls_back_to_the_control_plane() {
        let ours = [DataPlaneKind::SharedMemory];
        assert_eq!(negotiate(&ours, &[]), DataPlaneKind::ControlPlane);
        assert_eq!(negotiate(&ours, &ours), DataPlaneKind::SharedMemory);
    }

    #[tokio::test]
    async fn the_fallback_plane_refuses_frames() {
        let err = ControlPlaneOnly.send(b"x").await.err();
        assert_eq!(err.map(|e| e.kind()), Some(io::ErrorKind::Unsupported));
    }
}
