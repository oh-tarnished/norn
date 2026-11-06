//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How a [`Sink`](crate::Sink) behaves, each choice with its default.

use crate::health::Notice;
use norn_protocol::telemetry::AcknowledgementLevel;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// What is told of each [`Notice`]: a log line, a metric, an event signal.
pub(crate) type OnNotice = Arc<dyn Fn(&Notice) + Send + Sync>;

/// A sink's options. [`Default`] is meaningful: acknowledged at `PERSISTED`,
/// 64 MiB held at most, gRPC only, told to no one.
///
/// Plain data: `Send`, `Sync`, `Clone`; clones share one callback.
#[derive(Clone)]
pub struct Options {
    pub(crate) ack: AcknowledgementLevel,
    pub(crate) buffer_bytes: usize,
    pub(crate) shared_memory: bool,
    pub(crate) max_signals: usize,
    pub(crate) unary_limit: usize,
    pub(crate) connections: usize,
    pub(crate) drain: Duration,
    pub(crate) degraded_for: Duration,
    pub(crate) user_agent: String,
    pub(crate) on_notice: Option<OnNotice>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            ack: AcknowledgementLevel::Persisted,
            buffer_bytes: 64 << 20,
            shared_memory: false,
            max_signals: 1_000_000,
            unary_limit: 256 << 10,
            connections: 8,
            drain: Duration::from_secs(10),
            degraded_for: Duration::from_secs(60),
            user_agent: concat!("norn-sink/", env!("CARGO_PKG_VERSION")).to_owned(),
            on_notice: None,
        }
    }
}

impl fmt::Debug for Options {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("ack", &self.ack)
            .field("buffer_bytes", &self.buffer_bytes)
            .field("shared_memory", &self.shared_memory)
            .field("max_signals", &self.max_signals)
            .field("unary_limit", &self.unary_limit)
            .field("connections", &self.connections)
            .field("drain", &self.drain)
            .field("degraded_for", &self.degraded_for)
            .field("user_agent", &self.user_agent)
            .field("on_notice", &self.on_notice.is_some())
            .finish()
    }
}

impl Options {
    /// The level a reading counts as delivered at (SPEC §19). `DURABLE` and
    /// `PERSISTED` are required of the daemon at negotiation.
    #[must_use]
    pub fn ack(mut self, level: AcknowledgementLevel) -> Self {
        self.ack = level;
        self
    }

    /// The most bytes held, waiting or unacknowledged; each signal may hold a
    /// sixty-fourth of it, never under 64 KiB. Past either, readings are
    /// given up and counted: the bound is on memory, so it cannot be a wait.
    #[must_use]
    pub fn buffer_bytes(mut self, bytes: usize) -> Self {
        self.buffer_bytes = bytes;
        self
    }

    /// Asks the daemon for a shared-memory ring. Honoured only in a build with
    /// the `shared-memory` feature; otherwise a [`Notice::NoSharedMemory`] and
    /// gRPC. A daemon that grants none leaves the call on gRPC, transparently.
    #[must_use]
    pub fn shared_memory(mut self, on: bool) -> Self {
        self.shared_memory = on;
        self
    }

    /// The most signals declared; one more is [`crate::SinkError::TooManySignals`].
    #[must_use]
    pub fn max_signals(mut self, limit: usize) -> Self {
        self.max_signals = limit;
        self
    }

    /// The limit on one unary message of a daemon that publishes none: 1.0.0-rc.1
    /// takes 256 KiB (protocol.md, "Message limits"). A limit the daemon publishes
    /// wins over this. A batch create is cut to three quarters of it, and halved
    /// again if refused `RESOURCE_EXHAUSTED`.
    #[must_use]
    pub fn unary_limit(mut self, bytes: usize) -> Self {
        self.unary_limit = bytes;
        self
    }

    /// Connections provisioning spreads one-at-a-time creates over, at least
    /// one. Eight made about 2,000 creates a second where one made 800.
    #[must_use]
    pub fn connections(mut self, n: usize) -> Self {
        self.connections = n.max(1);
        self
    }

    /// How long [`Sink::close`](crate::Sink::close) waits for what is held to
    /// be acknowledged before giving the rest up.
    #[must_use]
    pub fn drain(mut self, wait: Duration) -> Self {
        self.drain = wait;
        self
    }

    /// How long [`Health::degraded`](crate::Health::degraded) stays true after
    /// the last reading given up.
    #[must_use]
    pub fn degraded_for(mut self, window: Duration) -> Self {
        self.degraded_for = window;
        self
    }

    /// The `user_agent` given at negotiation: the adapter's name and version.
    #[must_use]
    pub fn user_agent(mut self, agent: impl Into<String>) -> Self {
        self.user_agent = agent.into();
        self
    }

    /// Calls `f` with every [`Notice`]. It runs on the caller of `push` or of
    /// `run`, under no lock, and must not block.
    #[must_use]
    pub fn on_notice(mut self, f: impl Fn(&Notice) + Send + Sync + 'static) -> Self {
        self.on_notice = Some(Arc::new(f));
        self
    }
}
