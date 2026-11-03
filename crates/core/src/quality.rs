//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How far a sample can be trusted.
//!
//! A severity plus a substatus, both from small closed sets with stable
//! numeric codes. The vocabulary is deliberately generic (communication
//! failure, out of service, substituted): an adapter maps its protocol's
//! status flags onto it, and keeps the original flags in metadata if it needs
//! them. Nothing here names a protocol.

use crate::error::{Error, Result};

/// The coarse trust level of a sample. Its code is stable storage format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum Severity {
    /// The value is what the source measured, and the source vouches for it.
    #[default]
    Good = 0,
    /// The value is usable but qualified: stale, estimated or substituted.
    Uncertain = 1,
    /// The value must not be used as a measurement.
    Bad = 2,
}

/// Why a sample has the severity it has. Its code is stable storage format.
///
/// Any substatus may pair with any severity; the pairing is the adapter's
/// judgement (an overridden value is `Good` to one source, `Uncertain` to
/// another).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Substatus {
    /// No further detail.
    #[default]
    None = 0,
    /// The source could not be reached; the value is the last one received.
    LastKnownValue = 1,
    /// The source has not updated within its expected interval.
    Stale = 2,
    /// The value was entered or substituted by an operator.
    Substituted = 3,
    /// The source reports its output is overridden.
    Overridden = 4,
    /// The value was estimated or interpolated rather than measured.
    Estimated = 5,
    /// The source is deliberately out of service.
    OutOfService = 6,
    /// Communication with the source failed.
    CommunicationFailure = 7,
    /// The source device reports a fault.
    DeviceFailure = 8,
    /// The sensing element reports a fault.
    SensorFailure = 9,
    /// The value lies outside the source's measurable range.
    OutOfRange = 10,
    /// The source or its mapping is misconfigured.
    ConfigurationError = 11,
}

/// A sample's quality: [`Severity`] plus [`Substatus`].
///
/// `Default` is good with no substatus, the overwhelmingly common case.
/// Concurrency: `Copy` plain data. Performance: two bytes; the
/// [`Quality::code`] form is one `u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Quality {
    severity: Severity,
    substatus: Substatus,
}

impl Quality {
    /// Good, no substatus.
    pub const GOOD: Self = Self::new(Severity::Good, Substatus::None);

    /// A quality from its parts.
    #[must_use]
    pub const fn new(severity: Severity, substatus: Substatus) -> Self {
        Self {
            severity,
            substatus,
        }
    }

    /// The coarse trust level.
    #[must_use]
    pub const fn severity(self) -> Severity {
        self.severity
    }

    /// The detail.
    #[must_use]
    pub const fn substatus(self) -> Substatus {
        self.substatus
    }

    /// The stable `u16` form: severity in the high byte, substatus in the low.
    #[must_use]
    pub const fn code(self) -> u16 {
        ((self.severity as u16) << 8) | self.substatus as u16
    }

    /// Parses [`Quality::code`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuality`] for a code outside the closed set, so data
    /// from a newer Norn is refused rather than misread.
    pub const fn from_code(code: u16) -> Result<Self> {
        let severity = match code >> 8 {
            0 => Severity::Good,
            1 => Severity::Uncertain,
            2 => Severity::Bad,
            _ => return Err(Error::InvalidQuality { code }),
        };
        let substatus = match code & 0xff {
            0 => Substatus::None,
            1 => Substatus::LastKnownValue,
            2 => Substatus::Stale,
            3 => Substatus::Substituted,
            4 => Substatus::Overridden,
            5 => Substatus::Estimated,
            6 => Substatus::OutOfService,
            7 => Substatus::CommunicationFailure,
            8 => Substatus::DeviceFailure,
            9 => Substatus::SensorFailure,
            10 => Substatus::OutOfRange,
            11 => Substatus::ConfigurationError,
            _ => return Err(Error::InvalidQuality { code }),
        };
        Ok(Self::new(severity, substatus))
    }
}
