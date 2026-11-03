//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Connection limits: how many clients a server holds at once, what happens
//! to the next one, and how long a silent connection may live (SPEC §16, §41).

use std::time::Duration;

use crate::TransportError;

/// The largest accepted `max_connections`. Each connection holds a socket,
/// HTTP/2 buffers and a task; past this, a local daemon has a bug or an
/// abusive client, not a workload.
pub const MAX_CONNECTIONS_CEILING: usize = 65_536;

/// What [`Incoming`](crate::Incoming) does with a client that arrives while
/// `max_connections` are already open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Overflow {
    /// Stop accepting until a slot frees. Waiting clients queue in the
    /// kernel's listen backlog (bounded by the OS: `SOMAXCONN` for sockets,
    /// busy retries for named pipes) and are served in order. The default:
    /// a burst of local clients is delayed, not failed.
    #[default]
    Queue,
    /// Accept and immediately close. The client sees the connection drop and
    /// can retry or report; nothing waits on the server's behalf.
    Refuse,
}

/// Limits applied to every connection a server accepts.
///
/// Built with [`Limits::default`] and adjusted with the builder methods;
/// validated when the server binds. Plain data: `Send`, `Sync`, `Copy`.
///
/// ```
/// use std::time::Duration;
/// use norn_transport::{Limits, Overflow};
///
/// let limits = Limits::default()
///     .max_connections(8)
///     .overflow(Overflow::Refuse)
///     .idle_timeout(Some(Duration::from_secs(30)));
/// assert_eq!(limits.max(), 8);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    max_connections: usize,
    overflow: Overflow,
    idle_timeout: Option<Duration>,
}

impl Default for Limits {
    /// 64 connections, [`Overflow::Queue`], a 10-minute idle timeout.
    ///
    /// 64 matches `norn_core`'s default `max_clients` and comfortably
    /// exceeds the expected local clients (adapters, CLI, MCP server) while
    /// bounding memory. Ten minutes outlasts any sensible client keep-alive
    /// interval, so only abandoned connections are reaped.
    fn default() -> Self {
        Self {
            max_connections: 64,
            overflow: Overflow::Queue,
            idle_timeout: Some(Duration::from_secs(600)),
        }
    }
}

impl Limits {
    /// Sets the maximum number of simultaneously open connections.
    /// Must be between 1 and [`MAX_CONNECTIONS_CEILING`].
    #[must_use]
    pub fn max_connections(mut self, n: usize) -> Self {
        self.max_connections = n;
        self
    }

    /// Sets the policy for a client arriving at capacity.
    #[must_use]
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Sets how long a connection may go without a byte read or written
    /// before it is closed; `None` disables the timeout.
    ///
    /// Long-lived streaming RPCs that may be silent for longer must either
    /// send keep-alives (HTTP/2 PING frames count as traffic) or run on a
    /// server with a longer timeout.
    #[must_use]
    pub fn idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// The configured maximum number of connections.
    pub fn max(&self) -> usize {
        self.max_connections
    }

    /// The configured overflow policy.
    pub fn overflow_policy(&self) -> Overflow {
        self.overflow
    }

    /// The configured idle timeout.
    pub fn idle(&self) -> Option<Duration> {
        self.idle_timeout
    }

    /// Checks the limits are in range.
    pub fn validate(&self) -> Result<(), TransportError> {
        if self.max_connections == 0 {
            return Err(TransportError::InvalidLimits("max_connections is 0"));
        }
        if self.max_connections > MAX_CONNECTIONS_CEILING {
            return Err(TransportError::InvalidLimits(
                "max_connections exceeds MAX_CONNECTIONS_CEILING",
            ));
        }
        if self.idle_timeout.is_some_and(|d| d.is_zero()) {
            return Err(TransportError::InvalidLimits("idle_timeout is zero"));
        }
        Ok(())
    }
}
