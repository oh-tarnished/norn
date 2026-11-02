//! Validated human-readable names.

use crate::error::{Error, NameError, Result};

/// A non-empty, bounded, printable name such as `AHU-01` or
/// `SupplyAirTemperature`.
///
/// Invariants, checked once at construction: at least one byte, at most
/// [`Name::MAX_BYTES`] bytes of UTF-8, and no control characters, so a name is
/// always safe to print in a log line or a terminal. Uniqueness is not a
/// property of a name; the runtime enforces it where it matters.
///
/// Concurrency: immutable owned data; `Send + Sync`.
/// Performance: one heap allocation, no re-validation on clone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// The longest name accepted, in UTF-8 bytes. Long enough for any
    /// realistic object path, short enough that a million names stay small.
    pub const MAX_BYTES: usize = 255;

    /// Validates `name`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] with the [`NameError`] saying why.
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(Error::InvalidName {
                reason: NameError::Empty,
            });
        }
        if name.len() > Self::MAX_BYTES {
            return Err(Error::InvalidName {
                reason: NameError::TooLong(name.len()),
            });
        }
        if let Some((at, _)) = name.char_indices().find(|(_, c)| c.is_control()) {
            return Err(Error::InvalidName {
                reason: NameError::ControlCharacter(at),
            });
        }
        Ok(Self(name))
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
