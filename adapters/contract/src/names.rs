//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The words files use for value types and quality, the same in every format.
//!
//! A quality number this build does not know (a newer protocol's) is written as
//! its decimal number and read back as that number, so an unknown one survives.

use std::borrow::Cow;

use norn_protocol::common::value::Kind;
use norn_protocol::common::{QualitySeverity, QualitySubstatus, ValueType};

/// A value type's word: `double`, as `nornctl signals create` takes it.
#[must_use]
pub fn value_type_name(kind: ValueType) -> &'static str {
    match kind {
        ValueType::Unspecified => "unspecified",
        ValueType::Bool => "bool",
        ValueType::Int32 => "int32",
        ValueType::Int64 => "int64",
        ValueType::Uint32 => "uint32",
        ValueType::Uint64 => "uint64",
        ValueType::Float => "float",
        ValueType::Double => "double",
        ValueType::String => "string",
        ValueType::Bytes => "bytes",
        ValueType::Enum => "enum",
    }
}

/// The value type of a word from [`value_type_name`]; `None` for anything else,
/// `unspecified` included, since no signal may have it.
#[must_use]
pub fn parse_value_type(word: &str) -> Option<ValueType> {
    Some(match word {
        "bool" => ValueType::Bool,
        "int32" => ValueType::Int32,
        "int64" => ValueType::Int64,
        "uint32" => ValueType::Uint32,
        "uint64" => ValueType::Uint64,
        "float" => ValueType::Float,
        "double" => ValueType::Double,
        "string" => ValueType::String,
        "bytes" => ValueType::Bytes,
        "enum" => ValueType::Enum,
        _ => return None,
    })
}

/// The value type a value belongs to.
#[must_use]
pub fn value_type_of(kind: &Kind) -> ValueType {
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

/// A severity's word: `GOOD`, `UNCERTAIN`, `BAD`, or its number when unknown.
#[must_use]
pub fn severity_name(number: i32) -> Cow<'static, str> {
    match QualitySeverity::try_from(number) {
        Ok(QualitySeverity::Unspecified) | Err(_) => Cow::Owned(number.to_string()),
        Ok(known) => {
            let name = known.as_str_name();
            Cow::Borrowed(name.strip_prefix("QUALITY_SEVERITY_").unwrap_or(name))
        }
    }
}

/// The number of a word from [`severity_name`].
#[must_use]
pub fn parse_severity(word: &str) -> Option<i32> {
    let name = format!("QUALITY_SEVERITY_{word}");
    QualitySeverity::from_str_name(&name)
        .map(|s| s as i32)
        .or_else(|| word.parse().ok())
}

/// A substatus's word: `STALE`, empty for none, or its number when unknown.
#[must_use]
pub fn substatus_name(number: i32) -> Cow<'static, str> {
    match QualitySubstatus::try_from(number) {
        Ok(QualitySubstatus::Unspecified) => Cow::Borrowed(""),
        Err(_) => Cow::Owned(number.to_string()),
        Ok(known) => {
            let name = known.as_str_name();
            Cow::Borrowed(name.strip_prefix("QUALITY_SUBSTATUS_").unwrap_or(name))
        }
    }
}

/// The number of a word from [`substatus_name`].
#[must_use]
pub fn parse_substatus(word: &str) -> Option<i32> {
    if word.is_empty() {
        return Some(0);
    }
    let name = format!("QUALITY_SUBSTATUS_{word}");
    QualitySubstatus::from_str_name(&name)
        .map(|s| s as i32)
        .or_else(|| word.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quality_number_this_build_does_not_know_survives_as_its_number() {
        assert_eq!(parse_severity(&severity_name(99)), Some(99));
        assert_eq!(parse_substatus(&substatus_name(99)), Some(99));
        assert_eq!(parse_substatus(&substatus_name(0)), Some(0));
        assert_eq!(parse_severity(&severity_name(1)), Some(1));
        assert_eq!(severity_name(1), "GOOD");
        assert_eq!(parse_substatus(&substatus_name(2)), Some(2));
        assert_eq!(substatus_name(2), "STALE");
    }

    #[test]
    fn every_value_type_has_a_word_that_reads_back() {
        for n in 1..=10 {
            let kind = ValueType::try_from(n).unwrap();
            assert_eq!(parse_value_type(value_type_name(kind)), Some(kind));
        }
        assert_eq!(parse_value_type("unspecified"), None);
    }
}
