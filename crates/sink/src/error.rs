//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a caller of [`Sink`](crate::Sink) must fix. The daemon being away,
//! slow or draining is not here: that is expected, and is reported through
//! [`Health`](crate::Health) and [`Notice`](crate::Notice), never returned.

use norn_protocol::common::ValueType;
use std::fmt;

/// A mistake in what the adapter asked for. Match on the variant; each carries
/// what was wrong. Nothing is changed by a call that returns one.
///
/// Plain data: `Send`, `Sync`, `Clone`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SinkError {
    /// An ID Norn would refuse ([`crate::ids::is_valid`]).
    InvalidId {
        /// `stream_id`, `entity_id` or `signal_id`.
        field: &'static str,
        /// The ID as given.
        id: String,
    },
    /// A signal whose entity was never declared to this sink.
    UnknownEntity {
        /// The signal's full name.
        signal: String,
    },
    /// A signal declared again with another value type, which Norn fixes at
    /// creation because stored chunks are encoded for it.
    TypeChanged {
        /// The signal's full name.
        signal: String,
        /// The type it was declared with first.
        declared: ValueType,
        /// The type asked for now.
        now: ValueType,
    },
    /// More signals than [`Options::max_signals`](crate::Options::max_signals):
    /// the catalog is the one thing a sink holds per signal (SPEC §41).
    TooManySignals {
        /// The limit.
        limit: usize,
    },
    /// A reading of a signal never declared to this sink.
    Undeclared {
        /// The signal's full name.
        signal: String,
    },
    /// A reading whose value is not of its signal's type: Norn would refuse it.
    WrongType {
        /// The signal's full name.
        signal: String,
        /// The signal's type.
        declared: ValueType,
        /// The value's type.
        got: ValueType,
    },
    /// [`Sink::run`](crate::Sink::run) is already running for this sink.
    AlreadyRunning,
    /// The sink was closed: it takes nothing more and runs no more.
    Closed,
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { field, id } => {
                write!(
                    f,
                    "{field} {id:?}: must be 1 to 63 of [a-z0-9-], starting with a letter"
                )
            }
            Self::UnknownEntity { signal } => write!(f, "{signal}: its entity is not declared"),
            Self::TypeChanged {
                signal,
                declared,
                now,
            } => write!(
                f,
                "{signal}: declared {} and now {}; a value type never changes",
                declared.as_str_name(),
                now.as_str_name()
            ),
            Self::TooManySignals { limit } => write!(f, "more than {limit} signals"),
            Self::Undeclared { signal } => write!(f, "{signal}: a reading of an undeclared signal"),
            Self::WrongType {
                signal,
                declared,
                got,
            } => write!(
                f,
                "{signal}: a {} signal given {}",
                declared.as_str_name(),
                got.as_str_name()
            ),
            Self::AlreadyRunning => f.write_str("the sink is already running"),
            Self::Closed => f.write_str("the sink is closed"),
        }
    }
}

impl std::error::Error for SinkError {}
