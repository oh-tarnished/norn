//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a sink says about itself: whether Norn answers ([`Reach`]), how
//! delivery stands ([`Health`]), what happened worth a line ([`Notice`]), and
//! how a run ended ([`Report`]). A daemon going away is an expected event,
//! reported here, never an error return.

use crate::counts::Counts;
use std::fmt;

/// Whether Norn is taking readings. Changes are told through
/// [`Sink::reach`](crate::Sink::reach), a `watch` that keeps only the newest.
///
/// Plain data. [`Default`] is down, with nothing said yet: before the first
/// connection nothing is known.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reach {
    /// A session is open: negotiated, its stream made, its call open.
    pub up: bool,
    /// One line saying why, for a log or a status page.
    pub detail: String,
}

/// A snapshot of delivery, for a status page or a health check. Plain data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Health {
    /// Whether Norn answers.
    pub reach: Reach,
    /// Readings acknowledged at the level asked for.
    pub delivered: u64,
    /// Readings held: waiting, or sent and not yet acknowledged.
    pub held: u64,
    /// Readings given up for want of room ([`Counts::dropped`]).
    pub dropped: u64,
    /// A reading was given up within [`Options::degraded_for`](crate::Options::degraded_for).
    pub degraded: bool,
    /// Signals declared and not yet made: their readings wait.
    pub unmade: usize,
}

/// How [`Sink::run`](crate::Sink::run) ended. Plain data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Everything counted over the run.
    pub counts: Counts,
    /// Readings still held when the drain ran out, given up unsent.
    pub never_sent: usize,
}

/// Something a person should hear of. Matched by variant, never by its text;
/// [`Display`](fmt::Display) is a log line. Plain data.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Notice {
    /// Readings given up by one push: past the byte bound, or past their
    /// signal's share. Counted in [`Counts`] too.
    Dropped {
        /// Given up because the buffer was full, oldest first.
        overflowed: u64,
        /// Given up because their signal held its share.
        gated: u64,
        /// Bytes held afterwards.
        held_bytes: usize,
    },
    /// Norn became reachable or unreachable; the same as [`Reach`].
    Reach(Reach),
    /// The daemon is draining to stop (protocol 1.7): it answered what it
    /// admitted and ended the call. Not a failure: the rest is resent on the
    /// next call, and [`Reach`] does not turn unless that call cannot be had.
    Draining {
        /// Readings held for the next call.
        held: usize,
    },
    /// A batch was refused for a reason a retry may not cure; held, and sent
    /// again after a backoff.
    Refused {
        /// The batch, by its ID on its call.
        batch: i64,
        /// Norn's message.
        message: String,
    },
    /// A batch was admitted and can no longer reach the level asked; held and
    /// sent again.
    StoppedShort {
        /// The batch.
        batch: i64,
        /// Norn's message.
        message: String,
    },
    /// Readings Norn said can never be stored, given up; the rest went on.
    Invalid {
        /// How many.
        readings: u64,
        /// The first one's signal.
        first: String,
        /// Norn's reason for it.
        why: String,
    },
    /// A batch over a limit the daemon did not publish: smaller from now on.
    TooLarge {
        /// The batch's size in readings.
        readings: usize,
    },
    /// Norn refused to create or update a resource; its readings will be
    /// refused and counted as invalid.
    ResourceRefused {
        /// Norn's status.
        why: String,
    },
    /// Every signal declared so far is made: no reading waits on one.
    Provisioned {
        /// Signals made this run.
        signals: usize,
    },
    /// Which plane a call's batches travel: a ring or the socket.
    Plane {
        /// A shared-memory ring was granted.
        shared_memory: bool,
    },
    /// The sink closed with readings held and no run ever started: given up.
    NeverSent {
        /// How many.
        readings: usize,
    },
    /// Shared memory was asked for in a build without the `shared-memory`
    /// feature: appending over the socket.
    NoSharedMemory,
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dropped {
                overflowed,
                gated,
                held_bytes,
            } => write!(
                f,
                "{} readings dropped: {overflowed} with the buffer full, {gated} past their \
                 signal's share; {held_bytes} bytes held",
                overflowed + gated
            ),
            Self::Reach(r) => f.write_str(&r.detail),
            Self::Draining { held } => write!(
                f,
                "Norn is shutting down: it answered what it took; {held} readings held for the \
                 next call"
            ),
            Self::Refused { batch, message } => write!(f, "batch {batch} refused: {message}"),
            Self::StoppedShort { batch, message } => {
                write!(f, "batch {batch} stopped short: {message}")
            }
            Self::Invalid {
                readings,
                first,
                why,
            } => write!(f, "{readings} readings refused, first {first}: {why}"),
            Self::TooLarge { readings } => write!(f, "a batch of {readings} was too large"),
            Self::ResourceRefused { why } => write!(f, "Norn refused a resource: {why}"),
            Self::Provisioned { signals } => {
                write!(f, "{signals} signals made; no reading waits on one")
            }
            Self::Plane {
                shared_memory: true,
            } => f.write_str("appending through a shared-memory ring"),
            Self::Plane {
                shared_memory: false,
            } => f.write_str("appending over the socket"),
            Self::NeverSent { readings } => {
                write!(
                    f,
                    "closed with no run started: {readings} readings never sent"
                )
            }
            Self::NoSharedMemory => f.write_str(
                "shared memory asked for in a build without it (feature `shared-memory`); \
                 appending over the socket",
            ),
        }
    }
}
