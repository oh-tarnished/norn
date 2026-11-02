//! The registry's typed errors. Callers match; nothing compares messages.

use std::fmt;
use std::path::PathBuf;

/// The three kinds of resource the registry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs, reason = "the variant names are the documentation")]
pub enum ResourceKind {
    Entity,
    Signal,
    Stream,
}

/// Every way a registry operation can fail.
///
/// `#[non_exhaustive]`: match with a wildcard arm. `Send + Sync`.
#[derive(Debug)]
#[non_exhaustive]
pub enum MetadataError {
    /// The named resource does not exist.
    NotFound {
        /// Its kind.
        kind: ResourceKind,
        /// Its resource ID.
        id: String,
    },
    /// A resource with this ID exists.
    AlreadyExists {
        /// Its kind.
        kind: ResourceKind,
        /// Its resource ID.
        id: String,
    },
    /// A field's value is not acceptable.
    Invalid {
        /// The field, such as `signal_id`.
        field: &'static str,
        /// What is wrong.
        reason: String,
    },
    /// The caller's revision is not the resource's current one (AIP-154).
    RevisionMismatch {
        /// The resource's current revision.
        current: u64,
    },
    /// The resource holds children or data, and history is never deleted as a
    /// side effect (docs/reference/protocol.md).
    NotEmpty {
        /// Its kind.
        kind: ResourceKind,
        /// Its resource ID.
        id: String,
        /// What it holds, such as `signals`.
        holds: &'static str,
    },
    /// A configured count limit would be passed.
    LimitExceeded {
        /// The limit, such as `max_signals`.
        limit: &'static str,
        /// Its value.
        max: usize,
    },
    /// An operating-system call failed.
    Io {
        /// What was being done.
        op: &'static str,
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Another process (or another `Registry` here) holds the directory.
    Locked {
        /// The lock file.
        path: PathBuf,
    },
    /// The log is damaged somewhere other than its tail; opening it would lose
    /// acknowledged changes.
    Corrupt {
        /// The log file.
        path: PathBuf,
        /// Offset of the damage.
        offset: u64,
        /// What was wrong.
        what: &'static str,
    },
    /// The log was written by a newer Norn; refused rather than misread.
    UnsupportedVersion {
        /// The version found.
        found: u32,
    },
    /// An earlier write or fsync failed, so memory and disk may disagree. Every
    /// later mutation is refused until restart replays the log.
    Poisoned,
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { kind, id } => write!(f, "{kind:?} {id} not found"),
            Self::AlreadyExists { kind, id } => write!(f, "{kind:?} {id} already exists"),
            Self::Invalid { field, reason } => write!(f, "{field}: {reason}"),
            Self::RevisionMismatch { current } => {
                write!(f, "stale etag; the current revision is {current}")
            }
            Self::NotEmpty { kind, id, holds } => write!(f, "{kind:?} {id} still holds {holds}"),
            Self::LimitExceeded { limit, max } => write!(f, "{limit} of {max} reached"),
            Self::Io { op, path, source } => write!(f, "{op} {}: {source}", path.display()),
            Self::Locked { path } => write!(f, "another process holds {}", path.display()),
            Self::Corrupt { path, offset, what } => {
                write!(f, "{}: {what} at byte {offset}", path.display())
            }
            Self::UnsupportedVersion { found } => write!(f, "log version {found} is too new"),
            Self::Poisoned => f.write_str("an earlier write failed; restart the daemon"),
        }
    }
}

impl std::error::Error for MetadataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
