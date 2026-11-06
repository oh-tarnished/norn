//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One observation of one signal, as an adapter hands it over.

use crate::ids;
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, QualitySubstatus, Sample, Value, ValueType};
use prost_types::Timestamp;

/// A reading: a value of a declared signal at the source's time.
///
/// Its `observe_time` and `sequence` are what make a resend a duplicate Norn
/// stores once (protocol.md, "Values and samples"), so both are fixed when the
/// reading is pushed and kept across every resend. A reading with no
/// [`sequence`](Reading::sequence) of its own is given the sink's next count.
///
/// Plain data: `Send`, `Sync`, `Clone`.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub(crate) signal: String,
    pub(crate) observe_time: Timestamp,
    pub(crate) value: Kind,
    pub(crate) quality: Quality,
    pub(crate) sequence: Option<i64>,
}

impl Reading {
    /// `value` of the signal `signal` of `entity`, observed at `at`: a
    /// [`std::time::SystemTime`] or a [`Timestamp`]. Quality `GOOD`.
    #[must_use]
    pub fn new(
        entity: &str,
        signal: &str,
        at: impl Into<Timestamp>,
        value: impl Into<Kind>,
    ) -> Self {
        Self {
            signal: ids::signal_name(entity, signal),
            observe_time: at.into(),
            value: value.into(),
            quality: good(),
            sequence: None,
        }
    }

    /// Its quality. Quality never alters a value: a `BAD` reading is still
    /// history (protocol.md, "Values and samples").
    #[must_use]
    pub fn quality(mut self, severity: QualitySeverity, substatus: QualitySubstatus) -> Self {
        self.quality = Quality {
            severity: severity as i32,
            substatus: substatus as i32,
        };
        self
    }

    /// Its sequence, for a source that numbers its readings: kept as given.
    #[must_use]
    pub fn sequence(mut self, sequence: i64) -> Self {
        self.sequence = Some(sequence);
        self
    }

    /// The signal's full name.
    #[must_use]
    pub fn signal(&self) -> &str {
        &self.signal
    }

    /// The type its value is.
    #[must_use]
    pub fn value_type(&self) -> ValueType {
        type_of(&self.value)
    }

    /// The sample, with `sequence` when it has none of its own. Moves.
    pub(crate) fn into_sample(self, sequence: i64) -> Sample {
        Sample {
            signal: self.signal,
            observe_time: Some(self.observe_time),
            value: Some(Value {
                kind: Some(self.value),
            }),
            quality: Some(self.quality),
            sequence: self.sequence.unwrap_or(sequence),
        }
    }
}

/// `GOOD`, no substatus.
fn good() -> Quality {
    Quality {
        severity: QualitySeverity::Good as i32,
        substatus: QualitySubstatus::Unspecified as i32,
    }
}

/// The value type of `kind`.
#[must_use]
pub(crate) fn type_of(kind: &Kind) -> ValueType {
    match kind {
        Kind::BoolValue(_) => ValueType::Bool,
        Kind::Int32Value(_) => ValueType::Int32,
        Kind::Int64Value(_) => ValueType::Int64,
        Kind::Uint32Value(_) => ValueType::Uint32,
        Kind::Uint64Value(_) => ValueType::Uint64,
        Kind::FloatValue(_) => ValueType::Float,
        Kind::DoubleValue(_) => ValueType::Double,
        Kind::StringValue(_) => ValueType::String,
        Kind::BytesValue(_) => ValueType::Bytes,
        Kind::EnumValue(_) => ValueType::Enum,
    }
}
