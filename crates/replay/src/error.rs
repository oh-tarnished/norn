//! What a replay can refuse or fail with. Callers match, never read messages.

use std::fmt;
use std::sync::Arc;

use norn_query::QueryError;

/// Why a replay control or read failed.
///
/// `Clone`, since one failure reaches every subscriber. `#[non_exhaustive]`: match
/// with a wildcard arm. `Send + Sync`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ReplayError {
    /// The session's state forbids this.
    Conflict {
        /// The control refused.
        control: &'static str,
        /// The state it was refused in.
        state: &'static str,
    },
    /// No session has that name.
    NotFound,
    /// A session already has that name.
    AlreadyExists,
    /// `max_replay_sessions` exist already.
    TooManySessions {
        /// The limit.
        limit: usize,
    },
    /// The given etag is not the session's.
    EtagMismatch,
    /// A time lies outside the session's interval.
    OutsideInterval,
    /// Reading history failed; [`QueryError::Corrupt`] means damaged data.
    Query(Arc<QueryError>),
    /// The daemon is shutting down.
    ShuttingDown,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { control, state } => {
                write!(f, "cannot {control} a session that is {state}")
            }
            Self::NotFound => f.write_str("no such replay session"),
            Self::AlreadyExists => f.write_str("a replay session already has that name"),
            Self::TooManySessions { limit } => write!(f, "{limit} replay sessions exist already"),
            Self::EtagMismatch => f.write_str("the etag is not the session's current one"),
            Self::OutsideInterval => f.write_str("that time is outside the session's interval"),
            Self::Query(e) => write!(f, "{e}"),
            Self::ShuttingDown => f.write_str("the daemon is shutting down"),
        }
    }
}

impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Query(e) => Some(&**e),
            _ => None,
        }
    }
}

impl From<QueryError> for ReplayError {
    fn from(e: QueryError) -> Self {
        Self::Query(Arc::new(e))
    }
}
