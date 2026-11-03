//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Values, qualities, times and value kinds.

use std::fmt;

use norn_core::{Quality, Sample, Timestamp, Value, ValueKind};
use norn_protocol::common;
use norn_protocol::common::value::Kind;

/// Why a value could not cross between wire and core. Each names what the
/// caller got wrong; `Display` is the text a `SampleError` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConvertError {
    /// `Value.kind` is unset, or a kind this build does not know.
    MissingValue,
    /// `uint32_value` is outside 0 to 2^32 - 1.
    Uint32OutOfRange(i64),
    /// An enum `number` is negative.
    NegativeEnum(i32),
    /// A `sequence` is negative.
    NegativeSequence(i64),
    /// A required field is unset.
    Missing(&'static str),
    /// The severity is unset or unknown.
    BadSeverity(i32),
    /// The substatus is unknown to this build.
    BadSubstatus(i32),
    /// `observe_time` does not fit nanoseconds in an `i64`.
    TimeOutOfRange,
    /// The enum ordinal is above `i32::MAX`, which the wire cannot carry.
    EnumTooLarge(u32),
    /// The sequence is above `i64::MAX`, which the wire cannot carry.
    SequenceTooLarge(u64),
    /// The `ValueType` is unset or unknown.
    BadValueType(i32),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue => f.write_str("value is unset"),
            Self::Uint32OutOfRange(v) => write!(f, "uint32_value {v} is out of range"),
            Self::NegativeEnum(v) => write!(f, "enum number {v} is negative"),
            Self::NegativeSequence(v) => write!(f, "sequence {v} is negative"),
            Self::Missing(field) => write!(f, "{field} is required"),
            Self::BadSeverity(v) => write!(f, "severity {v} is not valid"),
            Self::BadSubstatus(v) => write!(f, "substatus {v} is not known"),
            Self::TimeOutOfRange => f.write_str("observe_time is out of range"),
            Self::EnumTooLarge(v) => write!(f, "enum ordinal {v} exceeds i32::MAX"),
            Self::SequenceTooLarge(v) => write!(f, "sequence {v} exceeds i64::MAX"),
            Self::BadValueType(v) => write!(f, "value_type {v} is not valid"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// A wire value as core's. `uint64_value` reinterprets the same 64 bits; a
/// `uint32_value` is range-checked; the enum `symbol` is display-only and
/// dropped. Moves strings and bytes.
///
/// # Errors
/// The [`ConvertError`] naming the problem.
pub fn value_from_wire(value: common::Value) -> Result<Value, ConvertError> {
    Ok(match value.kind.ok_or(ConvertError::MissingValue)? {
        Kind::BoolValue(v) => Value::Bool(v),
        Kind::Int32Value(v) => Value::I32(v),
        Kind::Int64Value(v) => Value::I64(v),
        Kind::Uint32Value(v) => {
            Value::U32(u32::try_from(v).map_err(|_| ConvertError::Uint32OutOfRange(v))?)
        }
        Kind::Uint64Value(v) => Value::U64(v as u64),
        Kind::FloatValue(v) => Value::F32(v),
        Kind::DoubleValue(v) => Value::F64(v),
        Kind::StringValue(v) => Value::String(v),
        Kind::BytesValue(v) => Value::Bytes(v),
        Kind::EnumValue(v) => {
            Value::Enum(u32::try_from(v.number).map_err(|_| ConvertError::NegativeEnum(v.number))?)
        }
    })
}

/// A core value as the wire's. Copies strings and bytes.
///
/// # Errors
/// [`ConvertError::EnumTooLarge`] above `i32::MAX`;
/// [`ConvertError::MissingValue`] for a kind this build cannot send.
pub fn value_to_wire(value: &Value) -> Result<common::Value, ConvertError> {
    let kind = match value {
        Value::Bool(v) => Kind::BoolValue(*v),
        Value::I32(v) => Kind::Int32Value(*v),
        Value::I64(v) => Kind::Int64Value(*v),
        Value::U32(v) => Kind::Uint32Value(i64::from(*v)),
        Value::U64(v) => Kind::Uint64Value(*v as i64),
        Value::F32(v) => Kind::FloatValue(*v),
        Value::F64(v) => Kind::DoubleValue(*v),
        Value::String(v) => Kind::StringValue(v.clone()),
        Value::Bytes(v) => Kind::BytesValue(v.clone()),
        Value::Enum(v) => Kind::EnumValue(common::EnumValue {
            number: i32::try_from(*v).map_err(|_| ConvertError::EnumTooLarge(*v))?,
            symbol: String::new(),
        }),
        _ => return Err(ConvertError::MissingValue),
    };
    Ok(common::Value { kind: Some(kind) })
}

/// A `ValueType` number as a kind; its number is the kind's tag.
///
/// # Errors
/// [`ConvertError::BadValueType`], `UNSPECIFIED` included.
pub fn kind_from_wire(number: i32) -> Result<ValueKind, ConvertError> {
    u8::try_from(number)
        .ok()
        .and_then(ValueKind::from_tag)
        .ok_or(ConvertError::BadValueType(number))
}

/// A kind as its `ValueType` number.
#[must_use]
pub fn kind_to_wire(kind: ValueKind) -> i32 {
    i32::from(kind.tag())
}

/// A wire quality as core's: severity one below the wire's number, substatus
/// equal to it.
///
/// # Errors
/// [`ConvertError::BadSeverity`] for `UNSPECIFIED` (PROTOCOL: a source that does
/// not know says `UNCERTAIN`) or unknown; [`ConvertError::BadSubstatus`].
pub fn quality_from_wire(q: common::Quality) -> Result<Quality, ConvertError> {
    let severity = u16::try_from(q.severity)
        .ok()
        .and_then(|s| s.checked_sub(1))
        .filter(|s| *s <= 2)
        .ok_or(ConvertError::BadSeverity(q.severity))?;
    let substatus = u16::try_from(q.substatus)
        .ok()
        .filter(|s| *s <= 0xff)
        .ok_or(ConvertError::BadSubstatus(q.substatus))?;
    Quality::from_code((severity << 8) | substatus)
        .map_err(|_| ConvertError::BadSubstatus(q.substatus))
}

/// A core quality as the wire's.
#[must_use]
pub fn quality_to_wire(q: Quality) -> common::Quality {
    let code = q.code();
    common::Quality {
        severity: i32::from(code >> 8) + 1,
        substatus: i32::from(code & 0xff),
    }
}

/// A `google.protobuf.Timestamp` as nanoseconds.
///
/// # Errors
/// [`ConvertError::TimeOutOfRange`] outside `i64` nanoseconds or for `nanos`
/// outside 0 to 999,999,999.
pub fn time_from_wire(t: prost_types::Timestamp) -> Result<Timestamp, ConvertError> {
    if !(0..1_000_000_000).contains(&t.nanos) {
        return Err(ConvertError::TimeOutOfRange);
    }
    t.seconds
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(i64::from(t.nanos)))
        .map(Timestamp::from_nanos)
        .ok_or(ConvertError::TimeOutOfRange)
}

/// A timestamp as `google.protobuf.Timestamp`.
#[must_use]
pub fn time_to_wire(t: Timestamp) -> prost_types::Timestamp {
    crate::info::timestamp(t)
}

/// A core sample as the wire's, naming its signal `signal`.
///
/// # Errors
/// [`ConvertError::SequenceTooLarge`] above `i64::MAX`; as [`value_to_wire`].
pub fn sample_to_wire(sample: &Sample, signal: String) -> Result<common::Sample, ConvertError> {
    let raw = sample.sequence.get();
    Ok(common::Sample {
        signal,
        observe_time: Some(time_to_wire(sample.timestamp)),
        value: Some(value_to_wire(&sample.value)?),
        quality: Some(quality_to_wire(sample.quality)),
        sequence: i64::try_from(raw).map_err(|_| ConvertError::SequenceTooLarge(raw))?,
    })
}
