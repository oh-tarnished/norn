//! Admission control: the bounded stream of connections a server accepts.

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::{Connection, Endpoint, Limits, Listener, Overflow, TransportError};

/// A bound endpoint with connection limits applied.
///
/// Create with [`Incoming::bind`]; call [`accept`](Incoming::accept) in a
/// loop. At most [`Limits::max`] connections are open at once; the next
/// client is queued or refused per [`Overflow`]. `bind` does I/O but never
/// spawns a task.
///
/// # Concurrency
///
/// `accept` takes `&mut self`, so one task accepts. The returned
/// [`Connection`]s are independent. [`tracker`](Incoming::tracker) and
/// [`abort_handle`](Incoming::abort_handle) hand out `Send + Sync` views for
/// other tasks.
///
/// # Shutdown
///
/// Dropping `Incoming` stops accepting and releases the endpoint (the Unix
/// socket file is removed if still ours). Open connections live on until
/// dropped, idle out, or [`abort_handle`](Incoming::abort_handle) is
/// cancelled.
pub struct Incoming {
    listener: Box<dyn Listener>,
    limits: Limits,
    slots: Arc<Semaphore>,
    abort: CancellationToken,
    refused: u64,
}

impl Incoming {
    /// Binds `endpoint` under `limits`. Fails if the limits are invalid, the
    /// OS lacks the transport, or the endpoint is unsafe or taken (see
    /// [`TransportError`]).
    pub async fn bind(endpoint: &Endpoint, limits: Limits) -> Result<Self, TransportError> {
        limits.validate()?;
        let listener = crate::bind(endpoint).await?;
        Ok(Self::with_listener(listener, limits))
    }

    /// Applies `limits` to an already bound listener, e.g. a custom
    /// [`Listener`] implementation. `limits` must already be valid; an
    /// invalid maximum is clamped into range.
    pub fn with_listener(listener: Box<dyn Listener>, limits: Limits) -> Self {
        let max = limits.max().clamp(1, crate::MAX_CONNECTIONS_CEILING);
        Self {
            listener,
            limits,
            slots: Arc::new(Semaphore::new(max)),
            abort: CancellationToken::new(),
            refused: 0,
        }
    }

    /// Waits for the next admitted connection.
    ///
    /// Under [`Overflow::Queue`], waits for a free slot before accepting.
    /// Under [`Overflow::Refuse`], accepts, and if no slot is free closes the
    /// newcomer at once and keeps waiting. Errors are the listener's accept
    /// errors; the caller decides whether to continue. Cancel-safe.
    pub async fn accept(&mut self) -> Result<Connection, TransportError> {
        loop {
            let slot = match self.limits.overflow_policy() {
                Overflow::Queue => Some(acquire(Arc::clone(&self.slots)).await?),
                Overflow::Refuse => None,
            };
            let io = self.listener.accept().await?;
            let slot = match slot {
                Some(slot) => slot,
                None => match Arc::clone(&self.slots).try_acquire_owned() {
                    Ok(slot) => slot,
                    Err(_) => {
                        self.refused = self.refused.saturating_add(1);
                        drop(io);
                        continue;
                    }
                },
            };
            let idle = self.limits.idle();
            return Ok(Connection::new(io, slot, idle, self.abort.clone()));
        }
    }

    /// The endpoint actually bound (an ephemeral TCP port is filled in).
    pub fn local_endpoint(&self) -> &Endpoint {
        self.listener.local_endpoint()
    }

    /// How many clients were refused at capacity under [`Overflow::Refuse`].
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// A view of how many connections are open, usable after `self` is
    /// moved or dropped.
    pub fn tracker(&self) -> ConnectionTracker {
        ConnectionTracker {
            slots: Arc::clone(&self.slots),
            max: self.limits.max().clamp(1, crate::MAX_CONNECTIONS_CEILING),
        }
    }

    /// A token that, once cancelled, makes every connection from this
    /// `Incoming` fail its next read or write. Used to bound a drain.
    pub fn abort_handle(&self) -> CancellationToken {
        self.abort.clone()
    }
}

/// Counts the open connections of one [`Incoming`].
///
/// `Clone + Send + Sync`; cheap to copy into a shutdown task.
#[derive(Clone, Debug)]
pub struct ConnectionTracker {
    slots: Arc<Semaphore>,
    max: usize,
}

impl ConnectionTracker {
    /// Open connections right now. Under [`Overflow::Queue`], an `Incoming`
    /// waiting in `accept` holds one slot, which is counted.
    pub fn active(&self) -> usize {
        self.max.saturating_sub(self.slots.available_permits())
    }

    /// Resolves once every connection (and the `Incoming` itself, if it
    /// holds a slot) has been dropped. Combine with a timeout to bound it.
    pub async fn closed(&self) {
        let n = u32::try_from(self.max).unwrap_or(u32::MAX);
        // Never closed, so `acquire_many` only returns Ok; the permits are
        // released again immediately.
        let _ = self.slots.acquire_many(n).await;
    }
}

/// Waits for a free slot. Takes the `Arc` by value so the future borrows
/// nothing from `Incoming`, keeping `accept` `Send` without `Incoming: Sync`.
async fn acquire(slots: Arc<Semaphore>) -> Result<OwnedSemaphorePermit, TransportError> {
    // The semaphore is never closed, so this cannot fail in practice; map
    // it anyway rather than panic.
    slots
        .acquire_owned()
        .await
        .map_err(|_| TransportError::InvalidLimits("connection slots closed"))
}
