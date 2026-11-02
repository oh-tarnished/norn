//! Points in time and per-stream ordering.

use crate::error::{Error, Result};

/// Nanoseconds since the Unix epoch, UTC.
///
/// An `i64` reaches roughly 1677-09-21 to 2262-04-11, which covers every
/// historical record a building or machine produces and leaves negative
/// values for pre-1970 data. Leap seconds are not represented: like Unix time,
/// a timestamp counts SI seconds as if every day had 86,400 of them.
///
/// The core never reads a clock (SPEC §28): a `Timestamp` is always handed in
/// by an adapter, a replay clock or a test.
///
/// Concurrency: `Copy` plain data. Performance: one integer; ordering is
/// integer ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The Unix epoch, 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: Self = Self(0);
    /// The earliest representable instant.
    pub const MIN: Self = Self(i64::MIN);
    /// The latest representable instant.
    pub const MAX: Self = Self(i64::MAX);

    /// A timestamp from nanoseconds since the epoch. Every `i64` is valid.
    #[must_use]
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Nanoseconds since the epoch.
    #[must_use]
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// A timestamp from whole seconds since the epoch.
    ///
    /// # Errors
    ///
    /// [`Error::Overflow`] when the instant lies outside the `i64` range.
    pub const fn from_secs(secs: i64) -> Result<Self> {
        match secs.checked_mul(1_000_000_000) {
            Some(n) => Ok(Self(n)),
            None => Err(Error::Overflow),
        }
    }

    /// This timestamp moved by `nanos` (negative moves earlier).
    ///
    /// # Errors
    ///
    /// [`Error::Overflow`] when the result leaves the representable range;
    /// it never wraps, because a wrapped timestamp silently reorders history.
    pub const fn checked_add_nanos(self, nanos: i64) -> Result<Self> {
        match self.0.checked_add(nanos) {
            Some(n) => Ok(Self(n)),
            None => Err(Error::Overflow),
        }
    }

    /// Nanoseconds from `earlier` to `self`; negative if `earlier` is later.
    ///
    /// # Errors
    ///
    /// [`Error::Overflow`] when the span does not fit an `i64` (only for
    /// instants more than ~292 years apart).
    pub const fn nanos_since(self, earlier: Self) -> Result<i64> {
        match self.0.checked_sub(earlier.0) {
            Some(n) => Ok(n),
            None => Err(Error::Overflow),
        }
    }
}

/// A monotonically increasing per-stream sequence number.
///
/// It orders samples that share a timestamp and lets a reader detect gaps and
/// duplicates after a crash or reconnect (SPEC §25 preserves it through
/// replay). Who assigns it, and its scope, is the runtime's decision; the core
/// only guarantees it never wraps.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Sequence(u64);

impl Sequence {
    /// The first sequence number, zero.
    pub const ZERO: Self = Self(0);

    /// Wraps a raw sequence number.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw sequence number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The successor of this sequence number.
    ///
    /// # Errors
    ///
    /// [`Error::Overflow`] at `u64::MAX`: a wrapped sequence would make new
    /// samples look older than every stored one.
    pub const fn next(self) -> Result<Self> {
        match self.0.checked_add(1) {
            Some(n) => Ok(Self(n)),
            None => Err(Error::Overflow),
        }
    }
}
