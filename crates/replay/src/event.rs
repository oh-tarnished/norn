//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The one event model of SPEC §27: live and replayed telemetry are the same thing.

use std::any::Any;
use std::sync::Arc;

use norn_core::{Sample, Timestamp};

/// The most events in one [`Batch`], so one slow subscriber costs a bounded amount.
pub const MAX_BATCH_EVENTS: usize = 1_024;

/// A change to a virtual clock, delivered in order with the samples.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClockKind {
    /// The clock started or resumed.
    Running,
    /// The clock was paused.
    Paused,
    /// The clock jumped.
    Seeked,
    /// The pace changed.
    SpeedChanged,
    /// The clock reached the end of the interval.
    Ended,
    /// The session was stopped. The subscription ends after this.
    Stopped,
}

/// What happened.
///
/// Concurrency: plain owned data.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Payload {
    /// An observation, exactly as it was ingested.
    Sample {
        /// The signal's resource name.
        signal: Arc<str>,
        /// The sample.
        sample: Sample,
    },
    /// What a signal held just before a seek's target, sent after
    /// [`ClockKind::Seeked`], from `StateAt`. Folding a subscription's samples and
    /// states, keeping each signal's last, gives the state at its clock across
    /// seeks. On the wire, `Event.state_sample`.
    State {
        /// The signal's resource name.
        signal: Arc<str>,
        /// The sample.
        sample: Sample,
    },
    /// A change to the clock.
    Clock(ClockKind),
}

/// One event, without its position: a subscription numbers its own.
///
/// Concurrency: plain owned data.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// The wall clock for live; the session's virtual clock for replay.
    pub clock_time: Timestamp,
    /// What happened.
    pub payload: Payload,
}

/// Something kept alive while events are undelivered: their memory charge.
pub type Hold = Arc<dyn Any + Send + Sync>;

/// Events delivered together, with what pays for them.
///
/// Concurrency: shared read-only between subscribers in an `Arc`. Memory: the charge
/// ends when the last subscriber lets go, so a slow one is paid for.
#[derive(Clone)]
pub struct Batch {
    /// The events, in order.
    pub events: Vec<Event>,
    pub(crate) _hold: Option<Hold>,
}

impl std::fmt::Debug for Batch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Batch")
            .field("events", &self.events.len())
            .finish_non_exhaustive()
    }
}

impl Batch {
    pub(crate) fn new(events: Vec<Event>, hold: Option<Hold>) -> Self {
        Self {
            events,
            _hold: hold,
        }
    }

    pub(crate) fn clock(kind: ClockKind, clock_time: Timestamp) -> Self {
        let payload = Payload::Clock(kind);
        Self::new(
            vec![Event {
                clock_time,
                payload,
            }],
            None,
        )
    }
}
