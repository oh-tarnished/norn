//! The core's typed error model.
//!
//! Every failure is a variant carrying the context a caller needs to act on
//! it: which limit, which field, which byte. Callers `match`; nothing in Norn
//! compares error messages, because a message is for humans and may change.

use std::fmt;

/// Every way a core operation can fail.
///
/// `#[non_exhaustive]` so later stages add variants without breaking callers:
/// a `match` must carry a wildcard arm.
///
/// Concurrency: plain data; `Send + Sync`, freely shared.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A bounded collection or field would grow past its configured limit.
    LimitExceeded {
        /// Which limit was hit.
        limit: Limit,
        /// The configured maximum.
        max: usize,
        /// The size the operation would have produced.
        actual: usize,
    },
    /// A name is empty, too long, or contains a control character.
    InvalidName {
        /// Why the name was refused.
        reason: NameError,
    },
    /// A byte form ended before the value it announced was complete.
    Truncated {
        /// Bytes the decoder needed from the start of the input.
        needed: usize,
        /// Bytes it was given.
        available: usize,
    },
    /// A value tag this build does not know. Stored data written by a newer
    /// Norn is refused rather than misread (SPEC §40).
    UnknownValueTag {
        /// The tag byte read.
        tag: u8,
    },
    /// A known value tag whose payload is not a valid encoding of its kind,
    /// such as a boolean byte other than 0 or 1.
    MalformedValue {
        /// The value's tag.
        tag: u8,
    },
    /// A string payload was not valid UTF-8.
    InvalidUtf8 {
        /// Offset of the payload within the input.
        offset: usize,
    },
    /// A quality code outside the closed set.
    InvalidQuality {
        /// The code read.
        code: u16,
    },
    /// A configuration value the builder refused.
    InvalidConfig {
        /// The offending field.
        field: ConfigField,
        /// Why it was refused.
        reason: ConfigError,
    },
    /// Arithmetic on a timestamp or sequence left its representable range.
    Overflow,
}

/// The bounded quantities [`Error::LimitExceeded`] can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Limit {
    /// Entries in one [`crate::Metadata`] map.
    MetadataEntries,
    /// Bytes in one metadata key.
    MetadataKeyBytes,
    /// Bytes in one metadata value.
    MetadataValueBytes,
    /// Bytes in a string or bytes [`crate::Value`] payload.
    ValuePayloadBytes,
}

/// Why a [`crate::Name`] was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NameError {
    /// The name was empty.
    Empty,
    /// The name exceeded [`crate::Name::MAX_BYTES`]; carries its length.
    TooLong(usize),
    /// The name contained a control character at this byte offset.
    ControlCharacter(usize),
}

/// The [`crate::Config`] fields the builder validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConfigField {
    /// [`crate::MemoryBudgets::total_bytes`].
    MemoryTotal,
    /// The sum of the per-subsystem memory budgets.
    MemorySubsystems,
    /// A per-subsystem budget or limit, by its builder method name.
    Named(&'static str),
}

/// Why a configuration value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConfigError {
    /// Zero, where zero would make the subsystem unable to work at all.
    Zero,
    /// The per-subsystem budgets sum past the total; carries the sum.
    ExceedsTotal(u64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { limit, max, actual } => {
                write!(f, "{limit:?} limit exceeded: {actual} > {max}")
            }
            Self::InvalidName { reason } => write!(f, "invalid name: {reason:?}"),
            Self::Truncated { needed, available } => {
                write!(f, "truncated: needed {needed} bytes, had {available}")
            }
            Self::UnknownValueTag { tag } => write!(f, "unknown value tag {tag:#04x}"),
            Self::MalformedValue { tag } => write!(f, "malformed value, tag {tag:#04x}"),
            Self::InvalidUtf8 { offset } => write!(f, "invalid UTF-8 at offset {offset}"),
            Self::InvalidQuality { code } => write!(f, "invalid quality code {code:#06x}"),
            Self::InvalidConfig { field, reason } => {
                write!(f, "invalid config {field:?}: {reason:?}")
            }
            Self::Overflow => f.write_str("arithmetic overflow"),
        }
    }
}

impl std::error::Error for Error {}

/// The core's result type.
pub type Result<T> = std::result::Result<T, Error>;
